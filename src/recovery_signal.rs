use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Waiter(Arc<Semaphore>);

impl Waiter {
    pub fn new() -> Self {
        Self(Arc::new(Semaphore::new(0)))
    }
    pub async fn wait(&self) {
        self.0.acquire().await.unwrap().forget();
    }
}

impl Default for Waiter {
    fn default() -> Self {
        Self::new()
    }
}

pub enum WaiterKind {
    RefreshRss,
    Test1,
    Test2,
}

pub struct RecoverySignal {
    waiters: Vec<Waiter>,
}

impl RecoverySignal {
    pub fn new(waiters_count: usize) -> Self {
        let mut waiters = Vec::with_capacity(waiters_count);
        for _ in 0..waiters_count {
            waiters.push(Waiter::new());
        }
        Self { waiters }
    }
    pub fn get_waiter(&self, waiter_kind: WaiterKind) -> Waiter {
        self.waiters[waiter_kind as usize].clone()
    }
    pub fn recover(&self) {
        for waiter in &self.waiters {
            if waiter.0.available_permits() == 0 {
                waiter.0.add_permits(1);
            }
        }
    }
}

#[tokio::test]
async fn test() {
    use futures::future::join;
    use std::time::Duration;
    let signals = RecoverySignal::new(3);
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
