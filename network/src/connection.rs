use parking_lot::RwLock;
use byteorder::{BigEndian, ByteOrder};
use std::net::SocketAddr;
use std::time::Duration;
use std::thread;
use std::convert::AsRef;
use std::sync::Arc;
use std::io::prelude::*;
use std::net::TcpStream;
use config;
use std::sync::mpsc::Receiver;

const TIMEOUT: u64 = 15;

pub type PeerPairs = Vec<(u32, SocketAddr, Arc<RwLock<Option<TcpStream>>>)>;

#[derive(Debug, PartialEq, Clone)]
pub enum Operation {
    BROADCAST = 0,
    SINGLE = 1,
    SUBTRACT = 2,
}

impl ::std::marker::Copy for Operation {}

pub struct Connection {
    pub id_card: u32,
    pub peers_pair: PeerPairs,
}

impl Connection {
    pub fn new(config: &config::SleepyConfig) -> Self {
        let peers = config.peers.as_ref().unwrap();
        let id_card = config.id_card.unwrap();
        let mut peers_pair = Vec::default();
        for peer in peers.iter() {
            let id_card: u32 = peer.id_card.unwrap();
            let addr = format!("{}:{}", peer.ip.clone().unwrap(), peer.port.unwrap());
            let addr = addr.parse::<SocketAddr>().unwrap();
            peers_pair.push((id_card, addr, Arc::new(RwLock::new(None))));
        }
        Connection {
            id_card,
            peers_pair,
        }
    }
}

pub fn do_connect(con: &Connection) {
    for &(_, addr, ref stream) in &con.peers_pair {
        let stream_lock = stream.clone();
        thread::spawn(move || loop {
                          {
                              let stream_opt = &mut *stream_lock.as_ref().write();
                              if stream_opt.is_none() {
                                  trace!("connet {:?}", addr);
                                  let stream = TcpStream::connect(addr).ok();
                                  *stream_opt = stream;
                              }

                              let mut need_reconnect = false;
                              if let Some(ref mut stream) = stream_opt.as_mut() {
                                  trace!("handshake with {:?}!", addr);
                                  let mut header = [0; 8];
                                  BigEndian::write_u64(&mut header, 0xDEADBEEF00000000 as u64);
                                  let res = stream.write(&header);
                                  if res.is_err() {
                                      warn!("handshake with {:?} error!", addr);
                                      need_reconnect = true;
                                  }
                              }

                              if need_reconnect {
                                  *stream_opt = None;
                              }
                          }

                          let ten_sec = Duration::from_secs(TIMEOUT);
                          thread::sleep(ten_sec);
                          trace!("after sleep retry connect {:?}!", addr);
                      });
    }
}

pub fn broadcast(con: &Connection, msg: Vec<u8>, origin: u32, operate: Operation) {
    let request_id = 0xDEADBEEF00000000 + msg.len() + 4;
    let mut encoded_request_id = [0; 8];
    BigEndian::write_u64(&mut encoded_request_id, request_id as u64);
    let mut encoded_origin = [0; 4];
    BigEndian::write_u32(&mut encoded_origin, con.id_card);
    let mut buf = Vec::new();
    buf.extend(&encoded_request_id);
    buf.extend(&encoded_origin);
    buf.extend(msg);
    let send_msg = move |stream: &Arc<RwLock<Option<TcpStream>>>| {
        let streams_lock = stream.clone();
        let stream_opt = &mut (*streams_lock.as_ref().write());
        if let Some(ref mut stream) = stream_opt.as_mut() {
            let _ = stream.write(&buf);
        }
    };
    let mut peers = vec![];
    for &(id_card, _, ref stream) in &con.peers_pair {
        if is_send(id_card, origin, operate) {
            peers.push(id_card);
            send_msg(stream);
        }
    }

    info!("{:?} broadcast msg to nodes {:?} {:?}",
          con.id_card,
          operate,
          peers);
}

pub fn is_send(id_card: u32, origin: u32, operate: Operation) -> bool {
    operate == Operation::BROADCAST || (operate == Operation::SINGLE && id_card == origin) ||
    (operate == Operation::SUBTRACT && origin != id_card)
}

pub fn start_client(config: &config::SleepyConfig, rx: Receiver<(u32, Operation, Vec<u8>)>) {
    let con = Connection::new(config);
    do_connect(&con);
    thread::spawn(move || {
        info!("start client!");
        loop {
            let (origin, op, msg) = rx.recv().unwrap();
            broadcast(&con, msg, origin, op);
        }
    });
}

#[cfg(test)]
mod test {
    use super::is_send;
    use super::Operation;
    #[test]
    fn is_seng_mag() {
        assert!(is_send(0, 0, Operation::BROADCAST));
        assert!(is_send(0, 1, Operation::BROADCAST));

        assert!(is_send(0, 0, Operation::SINGLE));
        assert!(!is_send(0, 1, Operation::SINGLE));

        assert!(!is_send(0, 0, Operation::SUBTRACT));
        assert!(is_send(0, 1, Operation::SUBTRACT));
    }
}