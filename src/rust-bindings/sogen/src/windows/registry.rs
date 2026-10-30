use sogen_sys::windows::ffi;

const REG_NONE: u32 = 0;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const REG_MULTI_SZ: u32 = 7;
const REG_QWORD: u32 = 11;

/// A typed registry value. Data that does not decode as its type, such as a string that is not valid UTF-16, keeps
/// its raw bytes as [`RegistryValue::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryValue {
    Binary(Vec<u8>),
    Dword(u32),
    ExpandString(String),
    MultiString(Vec<String>),
    None,
    Other { kind: u32, data: Vec<u8> },
    Qword(u64),
    String(String),
}

impl RegistryValue {
    pub(super) fn from_native(kind: u32, data: Vec<u8>) -> Self {
        let decoded = match kind {
            REG_NONE if data.is_empty() => Some(Self::None),
            REG_SZ => decode_string(&data).map(Self::String),
            REG_EXPAND_SZ => decode_string(&data).map(Self::ExpandString),
            REG_BINARY => return Self::Binary(data),
            REG_DWORD => <[u8; 4]>::try_from(data.as_slice())
                .ok()
                .map(|bytes| Self::Dword(u32::from_le_bytes(bytes))),
            REG_MULTI_SZ => decode_multi_string(&data).map(Self::MultiString),
            REG_QWORD => <[u8; 8]>::try_from(data.as_slice())
                .ok()
                .map(|bytes| Self::Qword(u64::from_le_bytes(bytes))),
            _ => None,
        };

        decoded.unwrap_or(Self::Other { kind, data })
    }

    /// The `REG_*` type.
    pub fn kind(&self) -> u32 {
        match self {
            Self::Binary(_) => REG_BINARY,
            Self::Dword(_) => REG_DWORD,
            Self::ExpandString(_) => REG_EXPAND_SZ,
            Self::MultiString(_) => REG_MULTI_SZ,
            Self::None => REG_NONE,
            Self::Other { kind, .. } => *kind,
            Self::Qword(_) => REG_QWORD,
            Self::String(_) => REG_SZ,
        }
    }

    /// The data as the registry stores it: strings in NUL-terminated UTF-16, numbers little-endian.
    pub fn data(&self) -> Vec<u8> {
        match self {
            Self::Binary(data) | Self::Other { data, .. } => data.clone(),
            Self::Dword(value) => value.to_le_bytes().to_vec(),
            Self::ExpandString(text) | Self::String(text) => {
                let mut data = Vec::with_capacity((text.len() + 1) * 2);
                encode_string(text, &mut data);
                data
            }
            Self::MultiString(texts) => {
                let mut data = Vec::new();
                for text in texts {
                    encode_string(text, &mut data);
                }
                data.extend_from_slice(&[0, 0]);
                data
            }
            Self::None => Vec::new(),
            Self::Qword(value) => value.to_le_bytes().to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    name: String,
    value: RegistryValue,
}

impl RegistryEntry {
    pub(super) fn from_native(data: ffi::RegistryData) -> Self {
        Self {
            name: data.name,
            value: RegistryValue::from_native(data.kind, data.data),
        }
    }

    /// Empty for the default value of a key.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &RegistryValue {
        &self.value
    }
}

fn utf16_units(data: &[u8]) -> Option<Vec<u16>> {
    if data.len() % 2 != 0 {
        return None;
    }

    Some(
        data.chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect(),
    )
}

fn decode_string(data: &[u8]) -> Option<String> {
    let units = utf16_units(data)?;
    let text = units.strip_suffix(&[0]).unwrap_or(&units);
    String::from_utf16(text).ok()
}

fn decode_multi_string(data: &[u8]) -> Option<Vec<String>> {
    let units = utf16_units(data)?;
    let units = units
        .strip_suffix(&[0, 0])
        .or_else(|| units.strip_suffix(&[0]))
        .unwrap_or(&units);
    if units.is_empty() {
        return Some(Vec::new());
    }

    units
        .split(|&unit| unit == 0)
        .map(|text| String::from_utf16(text).ok())
        .collect()
}

fn encode_string(text: &str, data: &mut Vec<u8>) {
    for unit in text.encode_utf16().chain([0]) {
        data.extend_from_slice(&unit.to_le_bytes());
    }
}
