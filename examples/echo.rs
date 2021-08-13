use std::io;
use std::time::Duration;

use whirl::net::TcpListener;
use whirl::time::delay_for;
use std::str;

fn main() {
    let rt = whirl::RUNTIME.foo();

    whirl::block_on(async {
        let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
        while let Ok(stream) = listener.accept().await {
            let task = whirl::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    // println!("prepared to read");
                    let bytes = stream.read(&mut buf).await.unwrap();
                    if bytes == 0 {
                        return;
                    }
                    // println!("{} read[{}]: {}", i, bytes, str::from_utf8(&buf[..bytes]).unwrap());
                    let write = stream.write(&buf[..bytes]).await.unwrap();
                }
            });
            task.detach();
        }
    });
}
