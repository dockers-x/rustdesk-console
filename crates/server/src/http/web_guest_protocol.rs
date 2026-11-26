//! Fields used by the guest gateway from the RustDesk rendezvous wire format.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub(super) struct Envelope {
    #[prost(message, optional, tag = "8")]
    pub punch: Option<Punch>,
    #[prost(message, optional, tag = "18")]
    pub request_relay: Option<RequestRelay>,
    #[prost(message, optional, tag = "19")]
    pub relay: Option<Relay>,
}

#[derive(Clone, PartialEq, Message)]
pub(super) struct Punch {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(int32, tag = "2")]
    pub nat: i32,
    #[prost(string, tag = "3")]
    pub key: String,
    #[prost(string, tag = "5")]
    pub token: String,
    #[prost(bool, tag = "8")]
    pub force_relay: bool,
}

#[derive(Clone, PartialEq, Message)]
pub(super) struct Relay {
    #[prost(string, tag = "2")]
    pub uuid: String,
    #[prost(bytes = "vec", tag = "5")]
    pub peer_key: Vec<u8>,
    #[prost(string, tag = "6")]
    pub refusal: String,
    #[prost(string, tag = "7")]
    pub version: String,
}

#[derive(Clone, PartialEq, Message)]
pub(super) struct RequestRelay {
    #[prost(string, tag = "2")]
    pub uuid: String,
    #[prost(string, tag = "6")]
    pub key: String,
}
