//! Keyboard data, independent of a renderer or windowing backend.
use serde::{Deserialize, Serialize};

// Explicit variants keep host key mapping exhaustive. Tables use the authored
// variant order, exactly matching Serde's derived enum indices and names, while
// sharing name lookup and unit-variant decoding instead of generating one arm
// per key for each serializer/deserializer instantiation.
macro_rules! keys {
    ($name:ident; $($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            const NAMES: &'static [&'static str] = &[$(stringify!($variant)),+];
            const VALUES: &'static [Self] = &[$(Self::$variant),+];
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_unit_variant(
                    stringify!($name),
                    *self as u32,
                    Self::NAMES[*self as usize],
                )
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Identifier;
                impl<'de> serde::de::Visitor<'de> for Identifier {
                    type Value = $name;

                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("variant identifier")
                    }

                    fn visit_u64<E: serde::de::Error>(self, index: u64) -> Result<$name, E> {
                        usize::try_from(index)
                            .ok()
                            .and_then(|index| $name::VALUES.get(index))
                            .copied()
                            .ok_or_else(|| E::invalid_value(
                                serde::de::Unexpected::Unsigned(index),
                                &format!("variant index 0 <= i < {}", $name::VALUES.len()).as_str(),
                            ))
                    }

                    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<$name, E> {
                        self.visit_bytes(value.as_bytes())
                    }

                    fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<$name, E> {
                        $name::NAMES.iter()
                            .position(|name| name.as_bytes() == value)
                            .map(|index| $name::VALUES[index])
                            .ok_or_else(|| E::unknown_variant(
                                &String::from_utf8_lossy(value), $name::NAMES,
                            ))
                    }
                }

                struct Variant($name);
                impl<'de> Deserialize<'de> for Variant {
                    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                        deserializer.deserialize_identifier(Identifier).map(Self)
                    }
                }

                struct EnumVisitor;
                impl<'de> serde::de::Visitor<'de> for EnumVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str(concat!("enum ", stringify!($name)))
                    }

                    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<$name, A::Error> {
                        let (Variant(value), unit) = data.variant::<Variant>()?;
                        serde::de::VariantAccess::unit_variant(unit)?;
                        Ok(value)
                    }
                }

                deserializer.deserialize_enum(stringify!($name), Self::NAMES, EnumVisitor)
            }
        }

        #[cfg(test)]
        impl $name {
            fn assert_derived_serde_parity() {
                #[allow(clippy::upper_case_acronyms, reason = "mirror authored wire names exactly")]
                #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
                enum Derived { $($variant),+ }

                for ((index, actual), expected) in Self::VALUES.iter().enumerate()
                    .zip([$(Derived::$variant),+])
                {
                    let bytes = crate::encode(actual);
                    assert_eq!(bytes, crate::encode(&expected));
                    assert_eq!(crate::decode::<Self>(&bytes).unwrap(), *actual);
                    assert_eq!(crate::decode::<Derived>(&bytes).unwrap(), expected);

                    // Enum identifiers accept authored names, their binary bytes,
                    // and unsigned indices, both bare and in singleton maps.
                    let name = Self::NAMES[index];
                    let from_name = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name);
                    assert_eq!(Self::deserialize(from_name).unwrap(), *actual);
                    let mut binary = vec![0xc5];
                    binary.extend_from_slice(&(name.len() as u16).to_be_bytes());
                    binary.extend_from_slice(name.as_bytes());
                    for identifier in [crate::encode(&(index as u64)), binary] {
                        for mapped in [false, true] {
                            let mut encoded = Vec::new();
                            if mapped { encoded.push(0x81); }
                            encoded.extend_from_slice(&identifier);
                            if mapped { encoded.push(0xc0); }
                            assert_eq!(crate::decode::<Self>(&encoded).unwrap(), *actual);
                            assert_eq!(crate::decode::<Derived>(&encoded).unwrap(), expected);
                        }
                    }
                }

                for invalid in [
                    crate::encode(&"not an authored key"),
                    crate::encode(&u64::MAX),
                    crate::encode(&(Self::VALUES.len() as u64)),
                    vec![0xc4, 1, 0xff], // invalid UTF-8 binary identifier
                    vec![0xd0, 0], // signed integer identifier
                    vec![0x81, 0, 1], // a unit variant with a non-unit payload
                    vec![0x92, 0, 0], // a sequence is not an enum
                ] {
                    assert!(crate::decode::<Derived>(&invalid).is_err());
                    assert!(crate::decode::<Self>(&invalid).is_err());
                }
            }
        }
    };
}

keys!(Named;
    Alt,
    AltGraph,
    CapsLock,
    Control,
    Fn,
    FnLock,
    NumLock,
    ScrollLock,
    Shift,
    Symbol,
    SymbolLock,
    Meta,
    Hyper,
    Super,
    Enter,
    Tab,
    Space,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    End,
    Home,
    PageDown,
    PageUp,
    Backspace,
    Clear,
    Copy,
    CrSel,
    Cut,
    Delete,
    EraseEof,
    ExSel,
    Insert,
    Paste,
    Redo,
    Undo,
    Accept,
    Again,
    Attn,
    Cancel,
    ContextMenu,
    Escape,
    Execute,
    Find,
    Help,
    Pause,
    Play,
    Props,
    Select,
    ZoomIn,
    ZoomOut,
    BrightnessDown,
    BrightnessUp,
    Eject,
    LogOff,
    Power,
    PowerOff,
    PrintScreen,
    Hibernate,
    Standby,
    WakeUp,
    AllCandidates,
    Alphanumeric,
    CodeInput,
    Compose,
    Convert,
    FinalMode,
    GroupFirst,
    GroupLast,
    GroupNext,
    GroupPrevious,
    ModeChange,
    NextCandidate,
    NonConvert,
    PreviousCandidate,
    Process,
    SingleCandidate,
    HangulMode,
    HanjaMode,
    JunjaMode,
    Eisu,
    Hankaku,
    Hiragana,
    HiraganaKatakana,
    KanaMode,
    KanjiMode,
    Katakana,
    Romaji,
    Zenkaku,
    ZenkakuHankaku,
    Soft1,
    Soft2,
    Soft3,
    Soft4,
    ChannelDown,
    ChannelUp,
    Close,
    MailForward,
    MailReply,
    MailSend,
    MediaClose,
    MediaFastForward,
    MediaPause,
    MediaPlay,
    MediaPlayPause,
    MediaRecord,
    MediaRewind,
    MediaStop,
    MediaTrackNext,
    MediaTrackPrevious,
    New,
    Open,
    Print,
    Save,
    SpellCheck,
    Key11,
    Key12,
    AudioBalanceLeft,
    AudioBalanceRight,
    AudioBassBoostDown,
    AudioBassBoostToggle,
    AudioBassBoostUp,
    AudioFaderFront,
    AudioFaderRear,
    AudioSurroundModeNext,
    AudioTrebleDown,
    AudioTrebleUp,
    AudioVolumeDown,
    AudioVolumeUp,
    AudioVolumeMute,
    MicrophoneToggle,
    MicrophoneVolumeDown,
    MicrophoneVolumeUp,
    MicrophoneVolumeMute,
    SpeechCorrectionList,
    SpeechInputToggle,
    LaunchApplication1,
    LaunchApplication2,
    LaunchCalendar,
    LaunchContacts,
    LaunchMail,
    LaunchMediaPlayer,
    LaunchMusicPlayer,
    LaunchPhone,
    LaunchScreenSaver,
    LaunchSpreadsheet,
    LaunchWebBrowser,
    LaunchWebCam,
    LaunchWordProcessor,
    BrowserBack,
    BrowserFavorites,
    BrowserForward,
    BrowserHome,
    BrowserRefresh,
    BrowserSearch,
    BrowserStop,
    AppSwitch,
    Call,
    Camera,
    CameraFocus,
    EndCall,
    GoBack,
    GoHome,
    HeadsetHook,
    LastNumberRedial,
    Notification,
    MannerMode,
    VoiceDial,
    TV,
    TV3DMode,
    TVAntennaCable,
    TVAudioDescription,
    TVAudioDescriptionMixDown,
    TVAudioDescriptionMixUp,
    TVContentsMenu,
    TVDataService,
    TVInput,
    TVInputComponent1,
    TVInputComponent2,
    TVInputComposite1,
    TVInputComposite2,
    TVInputHDMI1,
    TVInputHDMI2,
    TVInputHDMI3,
    TVInputHDMI4,
    TVInputVGA1,
    TVMediaContext,
    TVNetwork,
    TVNumberEntry,
    TVPower,
    TVRadioService,
    TVSatellite,
    TVSatelliteBS,
    TVSatelliteCS,
    TVSatelliteToggle,
    TVTerrestrialAnalog,
    TVTerrestrialDigital,
    TVTimer,
    AVRInput,
    AVRPower,
    ColorF0Red,
    ColorF1Green,
    ColorF2Yellow,
    ColorF3Blue,
    ColorF4Grey,
    ColorF5Brown,
    ClosedCaptionToggle,
    Dimmer,
    DisplaySwap,
    DVR,
    Exit,
    FavoriteClear0,
    FavoriteClear1,
    FavoriteClear2,
    FavoriteClear3,
    FavoriteRecall0,
    FavoriteRecall1,
    FavoriteRecall2,
    FavoriteRecall3,
    FavoriteStore0,
    FavoriteStore1,
    FavoriteStore2,
    FavoriteStore3,
    Guide,
    GuideNextDay,
    GuidePreviousDay,
    Info,
    InstantReplay,
    Link,
    ListProgram,
    LiveContent,
    Lock,
    MediaApps,
    MediaAudioTrack,
    MediaLast,
    MediaSkipBackward,
    MediaSkipForward,
    MediaStepBackward,
    MediaStepForward,
    MediaTopMenu,
    NavigateIn,
    NavigateNext,
    NavigateOut,
    NavigatePrevious,
    NextFavoriteChannel,
    NextUserProfile,
    OnDemand,
    Pairing,
    PinPDown,
    PinPMove,
    PinPToggle,
    PinPUp,
    PlaySpeedDown,
    PlaySpeedReset,
    PlaySpeedUp,
    RandomToggle,
    RcLowBattery,
    RecordSpeedNext,
    RfBypass,
    ScanChannelsToggle,
    ScreenModeNext,
    Settings,
    SplitScreenToggle,
    STBInput,
    STBPower,
    Subtitle,
    Teletext,
    VideoModeNext,
    Wink,
    ZoomToggle,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
);

keys!(Location; Standard, Left, Right, Numpad);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Named(Named),
    Character(String),
    Unidentified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeCode {
    Unidentified,
    Android(u32),
    MacOS(u16),
    Windows(u16),
    Xkb(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Physical {
    Unidentified(NativeCode),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub logo: bool,
    pub function: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyState {
    pub key: Key,
    pub modified_key: Key,
    pub physical_key: Physical,
    pub location: Location,
    pub modifiers: Modifiers,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Press {
        state: KeyState,
        text: Option<String>,
        repeat: bool,
    },
    Release(KeyState),
    Modifiers(Modifiers),
}

#[cfg(test)]
mod tests {
    #[test]
    fn named_keys_preserve_derived_serde_for_every_variant() {
        super::Named::assert_derived_serde_parity();
    }

    #[test]
    fn key_locations_preserve_derived_serde_for_every_variant() {
        super::Location::assert_derived_serde_parity();
    }
}
