mod activation;
mod aps;
mod auth;
pub mod cloudkit;
mod error;
pub mod facetime;
pub mod findmy;
mod ids;
pub mod imessage;
mod mmcs;
pub mod sharedstreams;
pub mod statuskit;
pub mod util;
pub use imessage::posterkit;
pub use util::KeyedArchive;

#[cfg(feature = "macos-validation-data")]
pub mod macos;

mod relay;

pub mod mmcsp {
    include!(concat!(env!("OUT_DIR"), "/mmcsp.rs"));
}

use std::collections::HashMap;
use std::fmt::Debug;

use activation::ActivationInfo;
pub use aps::{APSConnection, APSConnectionResource, APSMessage, APSState};
use async_trait::async_trait;
pub use auth::{
    authenticate_apple, authenticate_phone, login_apple_delegates, AuthPhone, LoginDelegate,
};
pub use cloudkit_derive;
pub use cloudkit_proto;
pub use error::PushError;
use icloud_auth::LoginClientInfo;
pub use ids::identity_manager::{DeliveryHandle, IdentityManager, MessageTarget, SendJob};
pub use ids::user::{
    register, IDSDeliveryData, IDSIdentity, IDSNGMIdentity, IDSUser, IDSUserIdentity,
    PrivateDeviceInfo, QueryOptions, ReportMessage, SupportAction, SupportAlert,
};
pub use ids::CertifiedContext;
pub use imessage::aps_client::{IMClient, MADRID_SERVICE};
pub use imessage::messages::{
    Attachment, AttachmentType, Balloon, BalloonLayout, ChangeParticipantMessage, ConversationData,
    DeleteTarget, EditMessage, ErrorMessage, AckMessage, ExtensionApp, IconChangeMessage, IndexedMessagePart,
    LPIconMetadata, LPImageMetadata, LPLinkMetadata, LinkMeta, MMCSFile, Message, MessageInst,
    MessagePart, MessageParts, MessageType, MoveToRecycleBinMessage, NormalMessage, OperatedChat,
    PartExtension, PermanentDeleteMessage, ReactMessage, ReactMessageType, Reaction, RenameMessage,
    RichLinkImageAttachmentSubstitute, ScheduleMode, SetTranscriptBackgroundMessage,
    ShareProfileMessage, SharedPoster, TextEffect, TextFlags, TextFormat, TypingApp, UnsendMessage,
    UpdateExtensionMessage, UpdateProfileMessage, UpdateProfileSharingMessage,
};
pub use imessage::name_photo_sharing;
pub use mmcs::{prepare_put, FileContainer};
pub use omnisette::AnisetteProvider;
use util::encode_hex;
pub use util::{NSArray, NSArrayClass, NSDictionaryClass, ResourceFailure, ResourceState, NSURL};

pub use auth::{
    AkData, ApsAlert, ApsData, CircleServerSession, IdmsAuthListener, IdmsCircleMessage,
    IdmsMessage, IdmsRequestedSignIn, TeardownSignIn,
};

use plist::Dictionary;
pub use relay::RelayConfig;
pub use util::get_gateways_for_mccmnc;

pub struct RegisterMeta {
    pub hardware_version: String,
    pub os_version: String,
    pub software_version: String,
}

pub struct DebugMeta {
    pub user_version: String,
    pub hardware_version: String,
    pub serial_number: String,
}

#[async_trait]
pub trait OSConfig: Sync + Send {
    fn build_activation_info(&self, csr: Vec<u8>) -> ActivationInfo;
    fn get_activation_device(&self) -> String;
    async fn generate_validation_data(&self) -> Result<Vec<u8>, PushError>;
    fn get_protocol_version(&self) -> u32;
    fn get_register_meta(&self) -> RegisterMeta;
    fn get_normal_ua(&self, item: &str) -> String;
    fn get_mme_clientinfo(&self, for_item: &str) -> String;
    fn get_version_ua(&self) -> String;
    fn get_device_name(&self) -> String;
    fn get_device_uuid(&self) -> String;
    fn get_private_data(&self) -> Dictionary;
    fn get_debug_meta(&self) -> DebugMeta;
    fn get_login_url(&self) -> &'static str;
    fn get_serial_number(&self) -> String;
    fn get_gsa_hardware_headers(&self) -> HashMap<String, String>;
    fn get_aoskit_version(&self) -> String;
    fn get_udid(&self) -> String {
        "55A1CFBF5BB56AD1159BD2CB7D6FF546E48EAAE4BF16188A07B1FB9C83138CA2".to_string()
    }

    fn get_adi_mme_info(&self, for_item: &str) -> String {
        self.get_mme_clientinfo(for_item)
    }

    fn get_gsa_config(&self, push: &APSState) -> LoginClientInfo {
        LoginClientInfo {
            ak_context_type: "imessage".to_string(),
            client_app_name: "Messages".to_string(),
            client_bundle_id: "com.apple.MobileSMS".to_string(),
            mme_client_info_akd: self.get_adi_mme_info("com.apple.AuthKit/1 (com.apple.akd/1.0)"),
            mme_client_info: self.get_adi_mme_info("com.apple.AuthKit/1 (com.apple.MobileSMS/1262.500.151.1.2)"),
            akd_user_agent: "akd/1.0 CFNetwork/1494.0.7 Darwin/23.4.0".to_string(),
            browser_user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)".to_string(),
            hardware_headers: self.get_gsa_hardware_headers(),
            push_token: push.token.map(|i| encode_hex(&i).to_uppercase()),
            update_account_bundle_id: self.get_adi_mme_info("com.apple.AppleAccount/1.0 (com.apple.systempreferences.AppleIDSettings/1)"),
        }
    }
}

extern crate log;
extern crate pretty_env_logger;
