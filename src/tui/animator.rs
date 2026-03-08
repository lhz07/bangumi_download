use crate::config_manager::SafeSend;
use crate::tui::events::LEvent;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::Interval;

impl SafeSend<AniCmd> for UnboundedSender<AniCmd> {
    fn send_msg(&self, msg: AniCmd) {
        if let Err(e) = self.send(msg) {
            log::error!("It seems that the Receiver of LEvent is closed too early, error: {e}");
        }
    }
}

pub struct AniSender {
    tx: UnboundedSender<AniCmd>,
}

impl AniSender {
    pub fn new(tx: UnboundedSender<AniCmd>) -> Self {
        Self { tx }
    }
    pub fn get_animator(&self) -> Animator {
        Animator::new(self.tx.clone())
    }
}

pub struct Animator {
    tx: UnboundedSender<AniCmd>,
}

impl Animator {
    fn new(tx: UnboundedSender<AniCmd>) -> Self {
        tx.send_msg(AniCmd::Start);
        Self { tx }
    }
}

impl Drop for Animator {
    fn drop(&mut self) {
        self.tx.send_msg(AniCmd::Stop);
    }
}

pub struct AnimationManager {
    current: u64,
    event_tx: UnboundedSender<LEvent>,
    rx: UnboundedReceiver<AniCmd>,
    sleeper: Sleeper,
}

pub enum AniCmd {
    Start,
    Stop,
}

impl AnimationManager {
    pub fn new(event_tx: UnboundedSender<LEvent>) -> (Self, AniSender) {
        let (tx, rx) = unbounded_channel();
        let sleeper = Sleeper::new(Duration::from_millis(50));
        let ani = AnimationManager {
            current: 0,
            event_tx,
            rx,
            sleeper,
        };
        (ani, AniSender::new(tx))
    }
    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(cmd) = self.rx.recv() => {
                    match cmd {
                        AniCmd::Start => self.current += 1,
                        AniCmd::Stop => self.current = self.current.saturating_sub(1),
                    }
                    if self.current > 0{
                        self.sleeper.enable();
                    }else{
                        self.sleeper.disable();
                        // render again to clear the screen
                        self.event_tx.send_msg(LEvent::Render);
                    }
                }
                _ = self.sleeper.sleep() => {
                    self.event_tx.send_msg(LEvent::Render);
                }
                else => {
                    break;
                }
            }
        }
    }
}

struct Sleeper {
    interval: Interval,
    enabled: bool,
}

impl Sleeper {
    fn new(period: Duration) -> Self {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            interval,
            enabled: false,
        }
    }
    fn enable(&mut self) {
        if !self.enabled {
            self.enabled = true;
            self.interval.reset();
        }
    }
    fn disable(&mut self) {
        if self.enabled {
            self.enabled = false;
        }
    }
    async fn sleep(&mut self) {
        if self.enabled {
            self.interval.tick().await;
        } else {
            futures::future::pending().await
        }
    }
}
