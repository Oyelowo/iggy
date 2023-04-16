use crate::handlers::response_handler::handle_status;
use crate::stream::Stream;
use std::io;
use std::str::from_utf8;
use tokio::net::UdpSocket;
use tracing::info;

const COMMAND: &[u8] = &[10];

pub async fn handle(socket: &UdpSocket, buffer: &mut [u8; 1024]) -> io::Result<()> {
    socket.send([COMMAND].concat().as_slice()).await?;
    let payload_length = socket.recv(buffer).await?;
    handle_status(buffer)?;

    if payload_length == 1 {
        info!("No streams found.");
        return Ok(());
    }

    let mut streams = Vec::new();
    let payload = &buffer[1..payload_length];
    let length = payload_length - 1;
    let mut position = 0;
    while position < length {
        let id = u32::from_le_bytes(payload[position..position + 4].try_into().unwrap());
        let topics = u32::from_le_bytes(payload[position + 4..position + 8].try_into().unwrap());
        let name_length =
            u32::from_le_bytes(payload[position + 8..position + 12].try_into().unwrap()) as usize;
        let name = from_utf8(&payload[position + 12..position + 12 + name_length]);
        streams.push(Stream {
            id,
            topics,
            name: name.unwrap().to_string(),
        });
        position += 4 + 4 + 4 + name_length;

        if position >= length {
            break;
        }
    }

    streams.sort_by(|x, y| x.id.cmp(&y.id));
    info!("Streams: {:#?}", streams);

    Ok(())
}