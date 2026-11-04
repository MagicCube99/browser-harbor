use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

const FIXTURE_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct TestListener {
    listener: TcpListener,
}

impl TestListener {
    pub fn bind() -> io::Result<(Self, SocketAddr)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        Ok((Self { listener }, address))
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
        self.accept_for(FIXTURE_TIMEOUT)
    }

    pub fn accept_for(&self, timeout: Duration) -> io::Result<TcpStream> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((socket, _)) => {
                    socket.set_nonblocking(false)?;
                    socket.set_read_timeout(Some(FIXTURE_TIMEOUT))?;
                    socket.set_write_timeout(Some(FIXTURE_TIMEOUT))?;
                    return Ok(socket);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "test client did not connect",
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub(crate) fn read_request(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buffer = [0; 8192];
    let read = socket.read(&mut buffer)?;
    Ok(buffer[..read].to_vec())
}

pub(crate) fn write_all(socket: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
    socket.write_all(bytes)
}
