use crate::socket_utils::SocketStream;

// macro_rules! println {
//     () => {
//         $crate::print!("\n")
//     };
//     ($($arg:tt)*) => {{
//         $crate::io::_print($crate::format_args_nl!($($arg)*));
//     }};
// }

// macro_rules! printf {
//     () => {
//         PRINT.print(format!("\n"))
//     };
//     ($($arg:tt)*) => {{
//         PRINT.print($crate::format_args_nl!($($arg)*));
//     }};
// }

pub trait WriteToOutput {
    fn print(&mut self, content: String);
}

impl WriteToOutput for SocketStream {
    fn print(&mut self, content: String) {
        // self.write_str(&content).await.unwrap();
        println!("{}", content);
    }
}

pub struct StdOut;

impl WriteToOutput for StdOut {
    fn print(&mut self, content: String) -> () {
        println!("{}", content);
    }
}
