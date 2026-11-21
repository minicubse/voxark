//! Bounded message transport for a trusted LAN. Connection IDs are transient;
//! persistent player credentials belong to the application handshake.
use renet::{ChannelConfig, ConnectionConfig, RenetClient, RenetServer, SendType};
use renet_netcode::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use std::{
    collections::VecDeque,
    fmt,
    net::{SocketAddr, UdpSocket},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub mod link;
mod mesher;
pub mod messages;
pub mod playtest;
pub use mesher::ClientMesher;
pub use messages::*;

pub const PROTOCOL_ID: u64 = 0x5052_494d_414c_0002;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);
impl ConnectionId {
    /// A transient identifier, generated independently from the persistent player token.
    pub fn random() -> std::io::Result<Self> {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Self(u64::from_le_bytes(bytes)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    Control = 0,
    Chunks = 1,
    Edits = 2,
    Input = 3,
    Snapshots = 4,
}
impl Channel {
    pub const ALL: [Self; 5] = [
        Self::Control,
        Self::Chunks,
        Self::Edits,
        Self::Input,
        Self::Snapshots,
    ];
    pub fn capacity(self) -> usize {
        match self {
            Self::Control => 2 * 1024 * 1024,
            Self::Chunks => 4 * 1024 * 1024,
            Self::Edits => 1024 * 1024,
            Self::Input => 64 * 1024,
            Self::Snapshots => 256 * 1024,
        }
    }
    pub fn reliable(self) -> bool {
        matches!(self, Self::Control | Self::Chunks | Self::Edits)
    }
}

pub fn connection_config() -> ConnectionConfig {
    let channels = Channel::ALL
        .into_iter()
        .map(|c| ChannelConfig {
            channel_id: c as u8,
            max_memory_usage_bytes: c.capacity(),
            send_type: if c.reliable() {
                SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(100),
                }
            } else {
                SendType::Unreliable
            },
        })
        .collect::<Vec<_>>();
    ConnectionConfig {
        available_bytes_per_tick: 256 * 1024,
        client_channels_config: channels.clone(),
        server_channels_config: channels,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrafficStats {
    /// Exact application payloads accepted by send, before packetization/ACKs.
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub sent_messages: u64,
    pub received_messages: u64,
    pub backpressure_events: u64,
}
impl TrafficStats {
    fn sent(&mut self, n: usize) {
        self.sent_bytes += n as u64;
        self.sent_messages += 1;
    }
    fn received(&mut self, n: usize) {
        self.received_bytes += n as u64;
        self.received_messages += 1;
    }
}

#[derive(Debug)]
pub enum TransportError {
    Io(std::io::Error),
    Transport(String),
    NotConnected,
    QueueFull(Channel),
    MessageTooLarge { bytes: usize, max: usize },
    InvalidMaxClients,
    EventQueueFull,
}
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "UDP: {e}"),
            Self::Transport(e) => write!(f, "transport: {e}"),
            Self::NotConnected => write!(f, "connection is not established"),
            Self::QueueFull(c) => write!(f, "{c:?} queue is full; retry later"),
            Self::MessageTooLarge { bytes, max } => {
                write!(f, "message has {bytes} bytes (maximum {max})")
            }
            Self::InvalidMaxClients => write!(f, "max_clients must be in 1..=64"),
            Self::EventQueueFull => write!(f, "drain server events before polling again"),
        }
    }
}
impl std::error::Error for TransportError {}
impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
fn transport(e: impl fmt::Display) -> TransportError {
    TransportError::Transport(e.to_string())
}
fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}
fn check_size(channel: Channel, bytes: usize) -> Result<(), TransportError> {
    // Unreliable messages should fit a single Renet packet; stale snapshots are replaceable.
    let max = if channel.reliable() {
        MAX_MESSAGE_BYTES
    } else {
        1100
    };
    if bytes > max {
        Err(TransportError::MessageTooLarge { bytes, max })
    } else {
        Ok(())
    }
}

pub struct ClientTransport {
    client: RenetClient,
    transport: NetcodeClientTransport,
    stats: TrafficStats,
    local_addr: SocketAddr,
}
impl ClientTransport {
    /// `id` must be freshly generated on every attempt, independently from player tokens.
    pub fn connect(
        bind: SocketAddr,
        server: SocketAddr,
        id: ConnectionId,
    ) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(bind)?;
        let local_addr = socket.local_addr()?;
        let authentication = ClientAuthentication::Unsecure {
            protocol_id: PROTOCOL_ID,
            client_id: id.0,
            server_addr: server,
            user_data: None,
        };
        Ok(Self {
            client: RenetClient::new(connection_config()),
            transport: NetcodeClientTransport::new(now(), authentication, socket)
                .map_err(transport)?,
            stats: TrafficStats::default(),
            local_addr,
        })
    }
    pub fn poll(&mut self, dt: Duration) -> Result<(), TransportError> {
        self.client.update(dt);
        self.transport
            .update(dt, &mut self.client)
            .map_err(transport)
    }
    pub fn recv(&mut self) -> Option<(Channel, Vec<u8>)> {
        for channel in Channel::ALL {
            if let Some(message) = self.client.receive_message(channel as u8) {
                self.stats.received(message.len());
                return Some((channel, message.to_vec()));
            }
        }
        None
    }
    pub fn send(
        &mut self,
        channel: Channel,
        message: impl Into<Vec<u8>>,
    ) -> Result<(), TransportError> {
        let message = message.into();
        check_size(channel, message.len())?;
        if !self.is_connected() {
            return Err(TransportError::NotConnected);
        }
        if !self.client.can_send_message(channel as u8, message.len()) {
            self.stats.backpressure_events += 1;
            return Err(TransportError::QueueFull(channel));
        }
        self.stats.sent(message.len());
        self.client.send_message(channel as u8, message);
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), TransportError> {
        self.transport
            .send_packets(&mut self.client)
            .map_err(transport)
    }
    pub fn connection_id(&self) -> ConnectionId {
        ConnectionId(self.transport.client_id())
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
    pub fn rtt(&self) -> Duration {
        Duration::from_secs_f64(self.client.rtt().max(0.0))
    }
    /// Renet packet payload rates, including retransmits, excluding netcode/UDP/IP headers.
    pub fn network_info(&self) -> renet::NetworkInfo {
        self.client.network_info()
    }
    pub fn stats(&self) -> TrafficStats {
        self.stats
    }
    pub fn disconnect_reason(&self) -> Option<String> {
        self.transport
            .disconnect_reason()
            .map(|r| r.to_string())
            .or_else(|| self.client.disconnect_reason().map(|r| r.to_string()))
    }
    pub fn disconnect(&mut self) {
        self.transport.disconnect();
        self.client.disconnect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    Connected(ConnectionId),
    Disconnected(ConnectionId, String),
}
pub struct ServerTransport {
    server: RenetServer,
    transport: NetcodeServerTransport,
    stats: TrafficStats,
    local_addr: SocketAddr,
    events: VecDeque<ServerEvent>,
}
impl ServerTransport {
    pub fn bind(addr: SocketAddr, max_clients: usize) -> Result<Self, TransportError> {
        if !(1..=64).contains(&max_clients) {
            return Err(TransportError::InvalidMaxClients);
        }
        let socket = UdpSocket::bind(addr)?;
        let local_addr = socket.local_addr()?;
        let config = ServerConfig {
            current_time: now(),
            max_clients,
            protocol_id: PROTOCOL_ID,
            public_addresses: vec![local_addr],
            authentication: ServerAuthentication::Unsecure,
        };
        Ok(Self {
            server: RenetServer::new(connection_config()),
            transport: NetcodeServerTransport::new(config, socket)?,
            stats: TrafficStats::default(),
            local_addr,
            events: VecDeque::new(),
        })
    }
    pub fn poll(&mut self, dt: Duration) -> Result<(), TransportError> {
        if self.events.len() >= 128 {
            return Err(TransportError::EventQueueFull);
        }
        self.server.update(dt);
        self.transport
            .update(dt, &mut self.server)
            .map_err(transport)?;
        while self.events.len() < 128 {
            let Some(event) = self.server.get_event() else {
                break;
            };
            self.events.push_back(match event {
                renet::ServerEvent::ClientConnected { client_id } => {
                    ServerEvent::Connected(ConnectionId(client_id))
                }
                renet::ServerEvent::ClientDisconnected { client_id, reason } => {
                    ServerEvent::Disconnected(ConnectionId(client_id), reason.to_string())
                }
            });
        }
        Ok(())
    }
    pub fn next_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }
    pub fn recv(&mut self) -> Option<(ConnectionId, Channel, Vec<u8>)> {
        for id in self.server.clients_id() {
            for channel in Channel::ALL {
                if let Some(message) = self.server.receive_message(id, channel as u8) {
                    self.stats.received(message.len());
                    return Some((ConnectionId(id), channel, message.to_vec()));
                }
            }
        }
        None
    }
    pub fn send(
        &mut self,
        id: ConnectionId,
        channel: Channel,
        message: impl Into<Vec<u8>>,
    ) -> Result<(), TransportError> {
        let message = message.into();
        check_size(channel, message.len())?;
        if !self.server.is_connected(id.0) {
            return Err(TransportError::NotConnected);
        }
        if !self
            .server
            .can_send_message(id.0, channel as u8, message.len())
        {
            self.stats.backpressure_events += 1;
            return Err(TransportError::QueueFull(channel));
        }
        self.stats.sent(message.len());
        self.server.send_message(id.0, channel as u8, message);
        Ok(())
    }
    /// Upstream server send_packets logs socket errors instead of returning them.
    /// Poll reports receive/transport errors; send reports size/backpressure/connection errors.
    pub fn flush(&mut self) -> Result<(), TransportError> {
        self.transport.send_packets(&mut self.server);
        Ok(())
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
    pub fn clients(&self) -> Vec<ConnectionId> {
        self.server
            .clients_id()
            .into_iter()
            .map(ConnectionId)
            .collect()
    }
    pub fn is_connected(&self, id: ConnectionId) -> bool {
        self.server.is_connected(id.0)
    }
    pub fn rtt(&self, id: ConnectionId) -> Option<Duration> {
        self.server
            .network_info(id.0)
            .ok()
            .map(|n| Duration::from_secs_f64(n.rtt.max(0.0)))
    }
    pub fn network_info(&self, id: ConnectionId) -> Option<renet::NetworkInfo> {
        self.server.network_info(id.0).ok()
    }
    pub fn stats(&self) -> TrafficStats {
        self.stats
    }
    pub fn disconnect(&mut self, id: ConnectionId) {
        self.server.disconnect(id.0);
    }
}
