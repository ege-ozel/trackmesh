use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::{request_response::Codec, StreamProtocol};
use protocol::{PeerRequest, PeerResponse, MAX_CHUNK_SIZE, MAX_MANIFEST_BYTES};
use serde::{de::DeserializeOwned, Serialize};
use std::io;

#[derive(Clone, Default)]
pub struct TransferCodec;

#[derive(Clone, Default)]
pub struct PairingCodec;

impl Codec for PairingCodec {
    type Protocol = StreamProtocol;
    type Request = protocol::PairingRequest;
    type Response = protocol::PairingResponse;
    async fn read_request<T: AsyncRead + Unpin + Send>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request> {
        read_json(io, protocol::MAX_PAIRING_BYTES)
            .await
            .map_err(|_| invalid("invalid pairing frame"))
    }
    async fn read_response<T: AsyncRead + Unpin + Send>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response> {
        read_json(io, protocol::MAX_PAIRING_BYTES)
            .await
            .map_err(|_| invalid("invalid pairing frame"))
    }
    async fn write_request<T: AsyncWrite + Unpin + Send>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        value: Self::Request,
    ) -> io::Result<()> {
        write_json(io, &value, protocol::MAX_PAIRING_BYTES).await?;
        io.close().await
    }
    async fn write_response<T: AsyncWrite + Unpin + Send>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        value: Self::Response,
    ) -> io::Result<()> {
        write_json(io, &value, protocol::MAX_PAIRING_BYTES).await?;
        io.close().await
    }
}
fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
async fn read_json<T: AsyncRead + Unpin + Send, V: DeserializeOwned>(
    io: &mut T,
    limit: usize,
) -> io::Result<V> {
    let data = read_frame(io, limit).await?;
    serde_json::from_slice(&data).map_err(invalid)
}
async fn read_frame<T: AsyncRead + Unpin + Send>(io: &mut T, limit: usize) -> io::Result<Vec<u8>> {
    let mut size = [0; 4];
    io.read_exact(&mut size).await?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > limit {
        return Err(invalid("frame length exceeds protocol limit"));
    }
    let mut data = vec![0; size];
    io.read_exact(&mut data).await?;
    Ok(data)
}
async fn write_json<T: AsyncWrite + Unpin + Send, V: Serialize>(
    io: &mut T,
    value: &V,
    limit: usize,
) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(invalid)?;
    write_frame(io, &data, limit).await
}
async fn write_frame<T: AsyncWrite + Unpin + Send>(
    io: &mut T,
    data: &[u8],
    limit: usize,
) -> io::Result<()> {
    if data.len() > limit {
        return Err(invalid("frame exceeds protocol limit"));
    }
    io.write_all(&(data.len() as u32).to_le_bytes()).await?;
    io.write_all(data).await
}
impl Codec for TransferCodec {
    type Protocol = StreamProtocol;
    type Request = PeerRequest;
    type Response = PeerResponse;
    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_frame(io, protocol::MAX_ENDPOINT_REQUEST_BYTES).await?;
        let request: PeerRequest = serde_json::from_slice(&data).map_err(invalid)?;
        if !matches!(&request, PeerRequest::EndpointUpdate { .. }) && data.len() > 16 * 1024 {
            return Err(invalid("request frame exceeds protocol limit"));
        }
        Ok(request)
    }
    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        request: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&request).map_err(invalid)?;
        let limit = if matches!(&request, PeerRequest::EndpointUpdate { .. }) {
            protocol::MAX_ENDPOINT_REQUEST_BYTES
        } else {
            16 * 1024
        };
        write_frame(io, &data, limit).await?;
        io.close().await
    }
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut tag = [0];
        io.read_exact(&mut tag).await?;
        match tag[0] {
            0 => {
                let response = read_json(io, MAX_MANIFEST_BYTES).await?;
                if matches!(response, PeerResponse::Chunk { .. }) {
                    return Err(invalid("chunks must use binary frames"));
                }
                Ok(response)
            }
            1 => {
                let mut hash = [0; 64];
                io.read_exact(&mut hash).await?;
                let hash = String::from_utf8(hash.to_vec()).map_err(invalid)?;
                if !protocol::valid_hash(&hash) {
                    return Err(invalid("invalid digest"));
                }
                let mut size = [0; 4];
                io.read_exact(&mut size).await?;
                let size = u32::from_le_bytes(size);
                if size == 0 || size > MAX_CHUNK_SIZE {
                    return Err(invalid("chunk exceeds limit"));
                }
                let mut data = vec![0; size as usize];
                io.read_exact(&mut data).await?;
                Ok(PeerResponse::Chunk { hash, data })
            }
            _ => Err(invalid("unknown response frame")),
        }
    }
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        response: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Chunks are binary: JSON integer arrays would waste memory, bandwidth, and CPU.
        match response {
            PeerResponse::Chunk { hash, data } => {
                if !protocol::valid_hash(&hash) || data.len() > MAX_CHUNK_SIZE as usize {
                    return Err(invalid("invalid chunk"));
                }
                io.write_all(&[1]).await?;
                io.write_all(hash.as_bytes()).await?;
                io.write_all(&(data.len() as u32).to_le_bytes()).await?;
                io.write_all(&data).await?;
            }
            value => {
                io.write_all(&[0]).await?;
                write_json(io, &value, MAX_MANIFEST_BYTES).await?;
            }
        }
        io.close().await
    }
}
