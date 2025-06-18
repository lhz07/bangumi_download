use tokio::net::UnixStream;

use crate::socket_utils::SocketStream;

// macro_rules! println {
//     () => {
//         $crate::print!("\n")
//     };
//     ($($arg:tt)*) => {{
//         $crate::io::_print($crate::format_args_nl!($($arg)*));
//     }};
// }

pub trait WriteToOutput{
    fn print(&mut self, content: String) -> impl std::future::Future<Output = ()> + Send;
}

impl WriteToOutput for SocketStream{
    async fn print(&mut self, content: String) {
        self.write_str(&content).await.unwrap();
    }
}

struct StdOut;

impl WriteToOutput for StdOut{
    async fn print(&mut self, content: String) -> () {
        println!("{}", content);
    }
}