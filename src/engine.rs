use crate::{
    ble,
    device::{Adapter, Candidate, Endpoint, Equalizer, Link, Sound, Speaker},
    protocol::{self, Decoder, cmd},
};
use std::{
    collections::VecDeque,
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc as async_mpsc,
    time::{self, timeout},
};

type Result<T> = std::result::Result<T, String>;
const REPLY_TIMEOUT: Duration = Duration::from_millis(850);
const TICK: Duration = Duration::from_secs(4);

trait Transport {
    fn details(&self) -> Link;
    async fn is_connected(&self) -> bool;
    async fn receive(&mut self) -> Option<Vec<u8>>;
    async fn write(&self, bytes: &[u8]) -> Result<()>;
}

#[derive(Clone, Debug)]
pub enum Control {
    Volume(u8),
    Source(u8),
    Sound(Sound, bool),
    Band(usize, u8),
    Preset(Equalizer),
    PresetName(Equalizer),
    ResetEq,
    Playback(u8),
    SpeakerName(String),
    PromptTones(bool),
    DisconnectAudio,
    PowerOff,
    FactoryReset,
}

pub enum Action {
    Discover { auto: bool, saved: Option<Endpoint> },
    Connect(Endpoint),
    Disconnect,
    Read,
    Set(Control),
    Shutdown,
}

#[derive(Clone, Default)]
pub struct State {
    pub candidates: Vec<Candidate>,
    pub adapter: Option<Adapter>,
    pub link: Option<Link>,
    pub speaker: Speaker,
    pub trace: VecDeque<String>,
    pub invalid_frames: usize,
    pub busy: Option<&'static str>,
    pub message: Option<String>,
    pub latency_ms: Option<u128>,
}

pub struct Worker {
    pub commands: async_mpsc::Sender<Action>,
    pub updates: mpsc::Receiver<State>,
    done: mpsc::Receiver<()>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.commands.try_send(Action::Shutdown).is_ok() {
            let _ = self.done.recv_timeout(Duration::from_millis(450));
        }
    }
}

pub fn spawn(redraw: impl Fn() + Send + 'static) -> Worker {
    let (cmd_tx, cmd_rx) = async_mpsc::channel(8);
    let (state_tx, state_rx) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    std::thread::Builder::new()
        .name("openconnex-ble".into())
        .spawn(move || {
            let start = || -> Result<()> {
                #[cfg(windows)]
                let _apartment = ble::Apartment::enter()?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime.block_on(event_loop(cmd_rx, &state_tx, &redraw));
                Ok(())
            };
            if let Err(message) = start() {
                let _ = state_tx.send(State {
                    message: Some(message),
                    ..State::default()
                });
                redraw();
            }
            let _ = done_tx.send(());
        })
        .expect("could not start Bluetooth worker");
    Worker {
        commands: cmd_tx,
        updates: state_rx,
        done,
    }
}

impl Transport for ble::Connection {
    fn details(&self) -> Link {
        self.details()
    }
    async fn is_connected(&self) -> bool {
        self.is_connected().await
    }
    async fn receive(&mut self) -> Option<Vec<u8>> {
        self.receive().await
    }
    async fn write(&self, bytes: &[u8]) -> Result<()> {
        self.write(bytes).await
    }
}

struct Session<T> {
    transport: T,
    decoder: Decoder,
    start: Instant,
}

impl<T: Transport> Session<T> {
    fn new(transport: T) -> Self {
        Self {
            transport,
            decoder: Decoder::default(),
            start: Instant::now(),
        }
    }

    fn log(&self, state: &mut State, direction: &str, bytes: &[u8]) {
        if state.trace.len() >= 100 {
            state.trace.pop_front();
        }
        state.trace.push_back(format!(
            "{:>8.3}s   {direction}   {}",
            self.start.elapsed().as_secs_f64(),
            protocol::hex(bytes, " ")
        ));
    }

    fn ingest(&mut self, state: &mut State, bytes: &[u8], wanted: Option<u8>) -> Option<Vec<u8>> {
        self.log(state, "RX", bytes);
        let mut reply = None;
        for packet in self.decoder.push(bytes) {
            if Some(packet.code) == wanted {
                reply = Some(packet.data.clone());
            }
            state.speaker.apply(&packet);
        }
        state.invalid_frames = self.decoder.invalid;
        reply
    }

    async fn request(&mut self, state: &mut State, code: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let now = Instant::now();
        let bytes = protocol::request(code, payload);
        self.log(state, "TX", &bytes);
        timeout(Duration::from_secs(4), self.transport.write(&bytes))
            .await
            .map_err(|_| "Bluetooth write to the speaker timed out.")??;
        let reply = timeout(REPLY_TIMEOUT, async {
            while let Some(bytes) = self.transport.receive().await {
                if let Some(reply) = self.ingest(state, &bytes, Some(code)) {
                    return Ok(reply);
                }
            }
            Err("The Bluetooth notification channel closed.".to_owned())
        })
        .await
        .map_err(|_| format!("No response to command 0x{code:02X}."))??;
        state.latency_ms = Some(now.elapsed().as_millis());
        Ok(reply)
    }

    async fn refresh(&mut self, state: &mut State) -> Result<()> {
        let queries = [
            cmd::VOLUME_READ,
            cmd::SOUND_READ,
            cmd::INPUT_READ,
            cmd::EQ_READ,
            cmd::A2DP,
            cmd::CODEC,
            cmd::HEADPHONES,
            cmd::TONE_READ,
        ];
        let mut success = false;
        for code in queries {
            if (code == cmd::HEADPHONES && state.speaker.cap(25) & 4 == 0)
                || (code == cmd::TONE_READ && state.speaker.cap(21) & 32 == 0)
            {
                continue;
            }
            success |= self.request(state, code, &[]).await.is_ok();
        }
        state.link = Some(self.transport.details());
        if !success {
            return Err("The speaker stopped responding.".into());
        }
        Ok(())
    }

    async fn identify(&mut self, state: &mut State) -> Result<()> {
        state.speaker = Speaker::default();
        state.trace.clear();
        state.latency_ms = None;
        self.request(state, cmd::CAPABILITIES, &[]).await?;
        self.request(state, cmd::SOUND_READ, &[]).await?;
        if !state.speaker.is_mr3() {
            return Err("This speaker did not report the MR3 control layout.".into());
        }
        for code in [cmd::VERSION, cmd::NAME, cmd::ADDRESS] {
            let _ = self.request(state, code, &[]).await;
        }
        self.refresh(state).await
    }

    async fn ack(&mut self, state: &mut State, code: u8, payload: &[u8]) -> Result<()> {
        let reply = self.request(state, code, payload).await?;
        if reply.first() != Some(&1) {
            return Err(format!(
                "The speaker rejected command 0x{code:02X} ({}).",
                protocol::hex(&reply, " ")
            ));
        }
        Ok(())
    }

    async fn control(&mut self, state: &mut State, control: Control) -> Result<()> {
        if !state.speaker.is_mr3() || !self.transport.is_connected().await {
            return Err("Connect an MR3 speaker first.".into());
        }
        match control {
            Control::Volume(value) if state.speaker.volume.is_some_and(|(max, _)| value <= max) => {
                self.ack(state, cmd::VOLUME_WRITE, &[value]).await?;
                self.request(state, cmd::VOLUME_READ, &[]).await?;
                if state.speaker.volume.map(|(_, level)| level) != Some(value) {
                    return Err("The speaker did not report the requested volume.".into());
                }
            }
            Control::Source(value @ 1..=2)
                if state.speaker.input.is_some_and(|(kind, _)| kind == 4) =>
            {
                self.ack(state, cmd::INPUT_WRITE, &[4, value]).await?;
                self.request(state, cmd::INPUT_READ, &[]).await?;
                if state.speaker.input != Some((4, value)) {
                    return Err("The speaker did not report the requested input.".into());
                }
            }
            Control::Sound(sound, reset) if sound.mode <= 2 && sound.layout == 1 => {
                self.ack(state, cmd::SOUND_WRITE, &sound.encode(reset))
                    .await?;
                self.request(state, cmd::SOUND_READ, &[]).await?;
                if !reset && state.speaker.sound != Some(sound) {
                    return Err("The speaker did not report the requested sound settings.".into());
                }
            }
            Control::Band(index, value)
                if index < 9 && value <= 12 && state.speaker.equalizer.is_some() =>
            {
                let [high, low] = crate::device::BANDS[index].to_be_bytes();
                self.ack(state, cmd::BAND_WRITE, &[0, index as u8, high, low, value])
                    .await?;
                self.request(state, cmd::EQ_READ, &[]).await?;
                if state.speaker.equalizer.as_ref().map(|eq| eq.gains[index]) != Some(value) {
                    return Err("The speaker did not report the requested EQ gain.".into());
                }
            }
            Control::Preset(preset) => {
                preset.validate()?;
                // This response carries the applied EQ table on MR3.
                let response = self.request(state, cmd::EQ_LOAD, &preset.payload()).await?;
                if response.first() == Some(&0) && response.len() == 1 {
                    return Err("The speaker rejected the EQ preset.".into());
                }
                self.request(state, cmd::EQ_READ, &[]).await?;
                if !state
                    .speaker
                    .equalizer
                    .as_ref()
                    .is_some_and(|eq| eq.gains == preset.gains && eq.name == preset.name)
                {
                    return Err("The speaker did not report the requested equalizer preset.".into());
                }
            }
            Control::PresetName(preset) => {
                preset.validate()?;
                let mut payload = preset.id.to_vec();
                payload.extend(preset.name.as_bytes());
                self.ack(state, cmd::EQ_NAME, &payload).await?;
                self.request(state, cmd::EQ_READ, &[]).await?;
                if state.speaker.equalizer.as_ref().map(|eq| eq.name.as_str())
                    != Some(preset.name.as_str())
                {
                    return Err("The speaker did not report the requested EQ name.".into());
                }
            }
            Control::ResetEq => {
                self.ack(state, cmd::EQ_RESET, &[]).await?;
                self.request(state, cmd::EQ_READ, &[]).await?;
            }
            Control::Playback(value @ (0 | 1 | 4 | 5)) if state.speaker.playback_available() => {
                self.ack(state, cmd::PLAYBACK, &[value]).await?;
            }
            Control::SpeakerName(name) if state.speaker.cap(3) > 0 => {
                let name = name.trim();
                if name.is_empty() || name.len() > 24 || name.chars().any(char::is_control) {
                    return Err("Use a Bluetooth name of 1–24 UTF-8 bytes.".into());
                }
                self.ack(state, cmd::NAME_WRITE, name.as_bytes()).await?;
                self.request(state, cmd::NAME, &[]).await?;
            }
            Control::PromptTones(enabled) if state.speaker.prompt_tones.is_some() => {
                // The switch is the first field of prompt-tone profile 2.
                let payload = [2, 1, u8::from(enabled)];
                let reply = self.request(state, cmd::TONE_WRITE, &payload).await?;
                if reply != payload && reply.as_slice() != [1] {
                    return Err("The speaker rejected the prompt-tone setting.".into());
                }
                self.request(state, cmd::TONE_READ, &[]).await?;
                if state.speaker.prompt_tones != Some(enabled) {
                    return Err(
                        "The speaker did not report the requested prompt-tone setting.".into(),
                    );
                }
            }
            Control::DisconnectAudio if state.speaker.cap(6) == 1 => {
                self.ack(state, cmd::AUDIO_DISCONNECT, &[]).await?;
                self.request(state, cmd::A2DP, &[]).await?;
            }
            Control::PowerOff if state.speaker.cap(10) == 1 => {
                self.ack(state, cmd::POWER_OFF, &[]).await?;
            }
            Control::FactoryReset if state.speaker.cap(17) & 32 != 0 => {
                self.ack(state, cmd::RESET, &[]).await?;
            }
            _ => return Err("The connected speaker does not support this action.".into()),
        }
        Ok(())
    }
}

async fn open(endpoint: Endpoint, state: &mut State) -> Result<Session<ble::Connection>> {
    let candidate = state
        .candidates
        .iter()
        .find(|candidate| candidate.endpoint == endpoint)
        .cloned();
    let transport = timeout(Duration::from_secs(18), async {
        let first = ble::Connection::open(endpoint, candidate.clone()).await;
        #[cfg(windows)]
        if let Err(error) = &first
            && error.contains("Windows denied Bluetooth access")
        {
            time::sleep(Duration::from_millis(650)).await;
            return ble::Connection::open(endpoint, candidate).await;
        }
        first
    })
    .await
    .map_err(|_| "The speaker did not accept a Bluetooth connection in time.".to_owned())??;
    let mut session = Session::new(transport);
    if let Err(error) = session.identify(state).await {
        session.transport.close().await;
        return Err(error);
    }
    Ok(session)
}

async fn event_loop(
    mut commands: async_mpsc::Receiver<Action>,
    updates: &mpsc::Sender<State>,
    redraw: &impl Fn(),
) {
    let mut state = State::default();
    let publish = |state: &State| {
        let _ = updates.send(state.clone());
        redraw();
    };
    let mut active: Option<Session<ble::Connection>> = None;
    let mut tick = time::interval(TICK);
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        let action = if let Some(session) = active.as_mut() {
            tokio::select! {
                biased;
                action = commands.recv() => action,
                bytes = session.transport.receive() => {
                    if let Some(bytes) = bytes {
                        session.ingest(&mut state, &bytes, None);
                        publish(&state);
                        continue;
                    }
                    Some(Action::Disconnect)
                }
                _ = tick.tick() => {
                    if session.transport.is_connected().await {
                        continue;
                    }
                    state.message = Some("Bluetooth control link lost. Reconnect to resume.".into());
                    Some(Action::Disconnect)
                }
            }
        } else {
            commands.recv().await
        };
        let Some(action) = action else { break };
        if !matches!(action, Action::Disconnect) {
            state.message = None;
        }
        state.busy = match action {
            Action::Discover { .. } => Some("Scanning"),
            Action::Connect(_) => Some("Connecting"),
            Action::Read => Some("Refreshing"),
            Action::Set(_) => Some("Applying"),
            Action::Disconnect | Action::Shutdown => None,
        };
        publish(&state);
        let result = match action {
            Action::Discover { auto, saved } => {
                if let Ok(Ok(adapter)) = timeout(Duration::from_secs(3), ble::adapter()).await {
                    state.adapter = Some(adapter);
                }
                match ble::scan(Duration::from_secs(4)).await {
                    Ok(candidates) => {
                        state.candidates = candidates;
                        if let Some(link) = state.link.as_ref()
                            && let Some(candidate) = state
                                .candidates
                                .iter()
                                .find(|device| device.endpoint == link.endpoint)
                            && let Some(session) = active.as_mut()
                        {
                            session.transport.update_candidate(candidate.clone());
                            state.link = Some(session.transport.details());
                        }
                        let mr3 = state
                            .candidates
                            .iter()
                            .filter(|device| device.mr3)
                            .collect::<Vec<_>>();
                        let only = (mr3.len() == 1).then(|| mr3[0].endpoint);
                        if auto
                            && active.is_none()
                            && let Some(endpoint) = saved.or(only)
                        {
                            state.busy = Some("Connecting");
                            publish(&state);
                            open(endpoint, &mut state)
                                .await
                                .map(|session| active = Some(session))
                        } else {
                            Ok(())
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Action::Connect(endpoint) => {
                if let Some(session) = active.take() {
                    session.transport.close().await;
                }
                state.link = None;
                state.speaker = Speaker::default();
                open(endpoint, &mut state)
                    .await
                    .map(|session| active = Some(session))
            }
            Action::Disconnect => {
                if let Some(session) = active.take() {
                    session.transport.close().await;
                }
                state.link = None;
                state.speaker = Speaker::default();
                Ok(())
            }
            Action::Read => match active.as_mut() {
                Some(session) => {
                    for code in [cmd::CAPABILITIES, cmd::VERSION, cmd::NAME, cmd::ADDRESS] {
                        let _ = session.request(&mut state, code, &[]).await;
                    }
                    session.refresh(&mut state).await
                }
                None => Err("Connect an MR3 speaker first.".into()),
            },
            Action::Shutdown => break,
            Action::Set(control) => match active.as_mut() {
                Some(session) => session.control(&mut state, control).await,
                None => Err("Connect an MR3 speaker first.".into()),
            },
        };
        state.busy = None;
        if let Err(error) = result {
            state.message = Some(error);
        }
        if active.is_none() {
            state.link = None;
        }
        publish(&state);
        tick.reset();
    }
    if let Some(session) = active {
        session.transport.close().await;
    }
}

pub fn hardware_check(path: &std::path::Path, verify_writes: bool) -> Result<()> {
    let mut report = String::new();
    let mut state = State::default();
    let run = || -> Result<()> {
        #[cfg(windows)]
        let _apartment = ble::Apartment::enter()?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?
            .block_on(async {
                state.adapter = ble::adapter().await.ok();
                state.candidates = ble::scan(Duration::from_secs(6)).await?;
                let mut advertising = state.candidates.iter().filter(|device| device.mr3);
                let endpoint = match (advertising.next(), advertising.next()) {
                    (Some(candidate), None) => candidate.endpoint,
                    (None, _) => crate::storage::Library::load()?
                        .last_speaker
                        .ok_or("No advertising MR3 or saved speaker for the hardware check.")?,
                    _ => return Err(
                        "More than one MR3 is advertising. Use one speaker for the hardware check."
                            .into(),
                    ),
                };
                let mut session = open(endpoint, &mut state).await?;
                let speaker = &state.speaker;
                use std::fmt::Write;
                let _ = writeln!(
                    report,
                    "Adapter: {:?}\nLink: {:?}\nSpeaker: {speaker:#?}",
                    state.adapter, state.link
                );
                if !verify_writes {
                    session.transport.close().await;
                    return Ok(());
                }
                let result = async {
                    let (_, volume) = speaker.volume.ok_or("No volume readback")?;
                    let sound = speaker.sound.ok_or("No acoustic tuning readback")?;
                    let eq = speaker.equalizer.clone().ok_or("No custom EQ readback")?;
                    let changed_volume = if volume > 0 { volume - 1 } else { 1 };
                    let attempt = session
                        .control(&mut state, Control::Volume(changed_volume))
                        .await
                        .and_then(|()| {
                            (state.speaker.volume.map(|(_, value)| value) == Some(changed_volume))
                                .then_some(())
                                .ok_or("Volume mismatch after write".into())
                        });
                    let restore = session.control(&mut state, Control::Volume(volume)).await;
                    restore?;
                    if state.speaker.volume.map(|(_, value)| value) != Some(volume) {
                        return Err("Could not verify original volume was restored".into());
                    }
                    attempt?;
                    let _ = writeln!(
                        report,
                        "Volume changed and restored ({volume} → {changed_volume} → {volume})"
                    );
                    let target_sound = Sound {
                        space: if sound.space < 4 { sound.space + 1 } else { 3 },
                        ..sound
                    };
                    let attempt = session
                        .control(&mut state, Control::Sound(target_sound, false))
                        .await;
                    session
                        .control(&mut state, Control::Sound(sound, false))
                        .await?;
                    attempt?;
                    let _ = writeln!(
                        report,
                        "Acoustic-space adjustment changed and restored ({} → {} → {})",
                        sound.space, target_sound.space, sound.space
                    );

                    let gain = eq.gains[0];
                    let new_gain = if gain < 12 { gain + 1 } else { gain - 1 };
                    let attempt = session
                        .control(&mut state, Control::Band(0, new_gain))
                        .await;
                    session.control(&mut state, Control::Preset(eq)).await?;
                    attempt?;
                    let _ = writeln!(
                        report,
                        "EQ band change confirmed and original full preset restored"
                    );

                    if let Some(enabled) = state.speaker.prompt_tones {
                        let attempt = session
                            .control(&mut state, Control::PromptTones(!enabled))
                            .await;
                        session
                            .control(&mut state, Control::PromptTones(enabled))
                            .await?;
                        attempt?;
                        let _ = writeln!(report, "Prompt-tone setting changed and restored");
                    }
                    Ok(())
                }
                .await;
                session.transport.close().await;
                result
            })
    };
    let result = {
        let mut run = run;
        run()
    };
    for entry in &state.trace {
        report.push_str(entry);
        report.push('\n');
    }
    report.push_str(&format!("Result: {result:?}\n"));
    std::fs::write(path, report).map_err(|error| error.to_string())?;
    result
}
