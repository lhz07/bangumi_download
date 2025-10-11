use crate::BROADCAST_TX;
use crate::config_manager::SafeSend;
use crate::socket_utils::ServerMsg;
use bincode::{Decode, Encode};
use once_cell::sync::Lazy;
use std::iter::zip;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use strum::{Display, EnumCount, VariantArray};
use tokio::sync::Semaphore;

#[derive(Encode, Decode, Debug, Clone)]
pub struct Waiting {
    pub waiting_count: usize,
    pub states: [bool; WaiterKind::COUNT],
}

pub struct Waiter {
    sema: Semaphore,
    is_waiting: AtomicBool,
}

impl Waiter {
    pub fn new() -> Self {
        let sema = Semaphore::new(0);
        let is_waiting = AtomicBool::new(false);
        Self { sema, is_waiting }
    }
    pub async fn wait(&self) {
        self.is_waiting
            .store(true, std::sync::atomic::Ordering::Relaxed);
        BROADCAST_TX.send_msg(ServerMsg::WaitingState(RECOVERY_SIGNAL.get_waiting_state()));
        self.sema.acquire().await.unwrap().forget();
        self.is_waiting
            .store(false, std::sync::atomic::Ordering::Relaxed);
        BROADCAST_TX.send_msg(ServerMsg::WaitingState(RECOVERY_SIGNAL.get_waiting_state()));
    }
    pub fn is_waiting(&self) -> bool {
        self.is_waiting.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for Waiter {
    fn default() -> Self {
        Self::new()
    }
}

pub static RECOVERY_SIGNAL: Lazy<RecoverySignal> = Lazy::new(RecoverySignal::new);

impl Default for Waiting {
    fn default() -> Self {
        Self {
            waiting_count: WaiterKind::COUNT,
            states: [true; WaiterKind::COUNT],
        }
    }
}
#[derive(Debug, Display, EnumCount, VariantArray)]
pub enum WaiterKind {
    #[strum(to_string = "Refresh RSS")]
    RefreshRss,
    Test1,
    Test2,
}

pub struct RecoverySignal {
    waiters: [Arc<Waiter>; WaiterKind::COUNT],
}

impl Default for RecoverySignal {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoverySignal {
    pub fn new() -> Self {
        let waiters = std::array::from_fn(|_| Arc::new(Waiter::new()));
        Self { waiters }
    }
    pub fn get_waiter(&self, waiter_kind: WaiterKind) -> Arc<Waiter> {
        self.waiters[waiter_kind as usize].clone()
    }
    pub fn get_waiting_state(&self) -> Waiting {
        let mut waiting_count = 0;
        let mut states = [false; WaiterKind::COUNT];
        for (waiter, state) in zip(&self.waiters, &mut states) {
            if waiter.is_waiting() {
                waiting_count += 1;
                *state = true;
            }
        }
        Waiting {
            waiting_count,
            states,
        }
    }
    pub fn recover(&self) {
        for waiter in &self.waiters {
            if waiter.sema.available_permits() == 0 {
                waiter.sema.add_permits(1);
            }
        }
    }
}

#[cfg(not(miri))]
#[tokio::test]
async fn test() {
    use futures::future::join;
    use std::time::Duration;
    let signals = RecoverySignal::new();
    let waiter1 = signals.get_waiter(WaiterKind::RefreshRss);
    let waiter2 = signals.get_waiter(WaiterKind::Test1);
    let waiter3 = signals.get_waiter(WaiterKind::Test2);
    let h1 = tokio::spawn(async move {
        println!("waiter 1 and 2 waiting");
        join(waiter1.wait(), waiter2.wait()).await;
        println!("waiter 1 and 2 done");
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!("send recover signal");
    signals.recover();
    signals.recover();
    h1.await.unwrap();
    println!("waiter 3 waiting");
    tokio::time::timeout(Duration::from_millis(100), waiter3.wait())
        .await
        .unwrap();
    println!("waiter 3 done");
    tokio::time::timeout(Duration::from_millis(100), waiter3.wait())
        .await
        .unwrap_err();
    println!("waiter 3 timeout");
}
