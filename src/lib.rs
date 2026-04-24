use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::{Arc, LazyLock, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use tiktoken::{CoreBPE, Rank};

const OPENAI_TIKTOKEN_VERSION: &str = "0.12.0";
const R50K_PAT_STR: &str =
    "'(?:[sdmt]|ll|ve|re)| ?\\p{L}++| ?\\p{N}++| ?[^\\s\\p{L}\\p{N}]++|\\s++$|\\s+(?!\\S)|\\s";
const CL100K_PAT_STR: &str = "'(?i:[sdmt]|ll|ve|re)|[^\\r\\n\\p{L}\\p{N}]?+\\p{L}++|\\p{N}{1,3}+| ?[^\\s\\p{L}\\p{N}]++[\\r\\n]*+|\\s++$|\\s*[\\r\\n]|\\s+(?!\\S)|\\s";
const O200K_PAT_STR: &str = "[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]*[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?|[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]+[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n/]*|\\s*[\\r\\n]+|\\s+(?!\\S)|\\s+";

static ENCODING_CACHE: LazyLock<Mutex<HashMap<String, Arc<LoadedEncoding>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const R50K_BASE_BPE: &[u8] = include_bytes!("data/r50k_base.tiktoken");
const P50K_BASE_BPE: &[u8] = include_bytes!("data/p50k_base.tiktoken");
const CL100K_BASE_BPE: &[u8] = include_bytes!("data/cl100k_base.tiktoken");
const O200K_BASE_BPE: &[u8] = include_bytes!("data/o200k_base.tiktoken");

#[repr(C)]
pub struct TokenBuffer {
    pub data: *mut u32,
    pub len: u64,
}

#[derive(Clone, Copy)]
enum SpecialMode {
    Disallow = 0,
    EncodeAsText = 1,
    AllowAll = 2,
    AllowList = 3,
}

impl TryFrom<u32> for SpecialMode {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Disallow),
            1 => Ok(Self::EncodeAsText),
            2 => Ok(Self::AllowAll),
            3 => Ok(Self::AllowList),
            _ => Err(format!("unknown special token mode: {value}")),
        }
    }
}

struct LoadedEncoding {
    core_bpe: CoreBPE,
    decoder: HashMap<Rank, Vec<u8>>,
    special_tokens: Vec<String>,
    special_token_set: HashSet<String>,
}

struct EncodingSpec {
    bpe_name: &'static str,
    bpe_data: &'static [u8],
    pat_str: &'static str,
    special_tokens: fn() -> HashMap<String, Rank>,
}

impl EncodingSpec {
    fn load(&self) -> Result<LoadedEncoding, String> {
        let mergeable_ranks = load_tiktoken_bpe(self.bpe_data, self.bpe_name)?;
        let special_tokens = (self.special_tokens)();
        let decoder = build_decoder(&mergeable_ranks, &special_tokens);

        let mut special_token_names = special_tokens.keys().cloned().collect::<Vec<_>>();
        special_token_names.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

        let core_bpe = CoreBPE::new::<_, _, std::vec::IntoIter<(String, (Rank, Rank))>>(
            mergeable_ranks,
            special_tokens.clone(),
            self.pat_str,
        )
        .map_err(|err| format!("failed to build tokenizer: {err}"))?;

        let special_token_set = special_token_names.iter().cloned().collect::<HashSet<_>>();

        Ok(LoadedEncoding {
            core_bpe,
            decoder,
            special_tokens: special_token_names,
            special_token_set,
        })
    }
}

fn encoding_spec(name: &str) -> Result<EncodingSpec, String> {
    match name {
        "gpt2" | "r50k_base" => Ok(EncodingSpec {
            bpe_name: "r50k_base.tiktoken",
            bpe_data: R50K_BASE_BPE,
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_r50k_base,
        }),
        "p50k_base" => Ok(EncodingSpec {
            bpe_name: "p50k_base.tiktoken",
            bpe_data: P50K_BASE_BPE,
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_p50k_base,
        }),
        "p50k_edit" => Ok(EncodingSpec {
            bpe_name: "p50k_base.tiktoken",
            bpe_data: P50K_BASE_BPE,
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_p50k_edit,
        }),
        "cl100k_base" => Ok(EncodingSpec {
            bpe_name: "cl100k_base.tiktoken",
            bpe_data: CL100K_BASE_BPE,
            pat_str: CL100K_PAT_STR,
            special_tokens: special_tokens_cl100k_base,
        }),
        "o200k_base" => Ok(EncodingSpec {
            bpe_name: "o200k_base.tiktoken",
            bpe_data: O200K_BASE_BPE,
            pat_str: O200K_PAT_STR,
            special_tokens: special_tokens_o200k_base,
        }),
        "o200k_harmony" => Ok(EncodingSpec {
            bpe_name: "o200k_base.tiktoken",
            bpe_data: O200K_BASE_BPE,
            pat_str: O200K_PAT_STR,
            special_tokens: special_tokens_o200k_harmony,
        }),
        _ => Err(format!("unknown encoding: {name}")),
    }
}

fn resolve_encoding_name_for_model(model_name: &str) -> Result<&'static str, String> {
    let exact = match model_name {
        "o1" => Some("o200k_base"),
        "o3" => Some("o200k_base"),
        "o4-mini" => Some("o200k_base"),
        "gpt-5" => Some("o200k_base"),
        "gpt-4.1" => Some("o200k_base"),
        "gpt-4o" => Some("o200k_base"),
        "gpt-4" => Some("cl100k_base"),
        "gpt-3.5-turbo" => Some("cl100k_base"),
        "gpt-3.5" => Some("cl100k_base"),
        "gpt-35-turbo" => Some("cl100k_base"),
        "davinci-002" => Some("cl100k_base"),
        "babbage-002" => Some("cl100k_base"),
        "text-embedding-ada-002" => Some("cl100k_base"),
        "text-embedding-3-small" => Some("cl100k_base"),
        "text-embedding-3-large" => Some("cl100k_base"),
        "text-davinci-003" => Some("p50k_base"),
        "text-davinci-002" => Some("p50k_base"),
        "text-davinci-001" => Some("r50k_base"),
        "text-curie-001" => Some("r50k_base"),
        "text-babbage-001" => Some("r50k_base"),
        "text-ada-001" => Some("r50k_base"),
        "davinci" => Some("r50k_base"),
        "curie" => Some("r50k_base"),
        "babbage" => Some("r50k_base"),
        "ada" => Some("r50k_base"),
        "code-davinci-002" => Some("p50k_base"),
        "code-davinci-001" => Some("p50k_base"),
        "code-cushman-002" => Some("p50k_base"),
        "code-cushman-001" => Some("p50k_base"),
        "davinci-codex" => Some("p50k_base"),
        "cushman-codex" => Some("p50k_base"),
        "text-davinci-edit-001" => Some("p50k_edit"),
        "code-davinci-edit-001" => Some("p50k_edit"),
        "text-similarity-davinci-001" => Some("r50k_base"),
        "text-similarity-curie-001" => Some("r50k_base"),
        "text-similarity-babbage-001" => Some("r50k_base"),
        "text-similarity-ada-001" => Some("r50k_base"),
        "text-search-davinci-doc-001" => Some("r50k_base"),
        "text-search-curie-doc-001" => Some("r50k_base"),
        "text-search-babbage-doc-001" => Some("r50k_base"),
        "text-search-ada-doc-001" => Some("r50k_base"),
        "code-search-babbage-code-001" => Some("r50k_base"),
        "code-search-ada-code-001" => Some("r50k_base"),
        "gpt2" => Some("gpt2"),
        "gpt-2" => Some("gpt2"),
        _ => None,
    };
    if let Some(name) = exact {
        return Ok(name);
    }

    for (prefix, encoding_name) in [
        ("o1-", "o200k_base"),
        ("o3-", "o200k_base"),
        ("o4-mini-", "o200k_base"),
        ("gpt-5-", "o200k_base"),
        ("gpt-4.5-", "o200k_base"),
        ("gpt-4.1-", "o200k_base"),
        ("chatgpt-4o-", "o200k_base"),
        ("gpt-4o-", "o200k_base"),
        ("gpt-4-", "cl100k_base"),
        ("gpt-3.5-turbo-", "cl100k_base"),
        ("gpt-35-turbo-", "cl100k_base"),
        ("gpt-oss-", "o200k_harmony"),
        ("ft:gpt-4o", "o200k_base"),
        ("ft:gpt-4", "cl100k_base"),
        ("ft:gpt-3.5-turbo", "cl100k_base"),
        ("ft:davinci-002", "cl100k_base"),
        ("ft:babbage-002", "cl100k_base"),
    ] {
        if model_name.starts_with(prefix) {
            return Ok(encoding_name);
        }
    }

    Err(format!(
        "could not automatically map {model_name} to a tokeniser. Please use get_encoding explicitly."
    ))
}

fn special_tokens_r50k_base() -> HashMap<String, Rank> {
    HashMap::from([(String::from("<|endoftext|>"), 50256)])
}

fn special_tokens_p50k_base() -> HashMap<String, Rank> {
    HashMap::from([(String::from("<|endoftext|>"), 50256)])
}

fn special_tokens_p50k_edit() -> HashMap<String, Rank> {
    HashMap::from([
        (String::from("<|endoftext|>"), 50256),
        (String::from("<|fim_prefix|>"), 50281),
        (String::from("<|fim_middle|>"), 50282),
        (String::from("<|fim_suffix|>"), 50283),
    ])
}

fn special_tokens_cl100k_base() -> HashMap<String, Rank> {
    HashMap::from([
        (String::from("<|endoftext|>"), 100257),
        (String::from("<|fim_prefix|>"), 100258),
        (String::from("<|fim_middle|>"), 100259),
        (String::from("<|fim_suffix|>"), 100260),
        (String::from("<|endofprompt|>"), 100276),
    ])
}

fn special_tokens_o200k_base() -> HashMap<String, Rank> {
    HashMap::from([
        (String::from("<|endoftext|>"), 199999),
        (String::from("<|endofprompt|>"), 200018),
    ])
}

fn special_tokens_o200k_harmony() -> HashMap<String, Rank> {
    let mut special_tokens = special_tokens_o200k_base();
    for (token, rank) in [
        ("<|startoftext|>", 199998),
        ("<|endoftext|>", 199999),
        ("<|reserved_200000|>", 200000),
        ("<|reserved_200001|>", 200001),
        ("<|return|>", 200002),
        ("<|constrain|>", 200003),
        ("<|reserved_200004|>", 200004),
        ("<|channel|>", 200005),
        ("<|start|>", 200006),
        ("<|end|>", 200007),
        ("<|message|>", 200008),
        ("<|reserved_200009|>", 200009),
        ("<|reserved_200010|>", 200010),
        ("<|reserved_200011|>", 200011),
        ("<|call|>", 200012),
    ] {
        special_tokens.insert(token.to_string(), rank);
    }
    for rank in 200013..201088 {
        special_tokens.insert(format!("<|reserved_{rank}|>"), rank);
    }
    special_tokens
}

fn get_encoding(name: &str) -> Result<Arc<LoadedEncoding>, String> {
    let mut cache = ENCODING_CACHE
        .lock()
        .map_err(|_| String::from("encoding cache mutex poisoned"))?;

    if let Some(existing) = cache.get(name) {
        return Ok(Arc::clone(existing));
    }

    let loaded = Arc::new(encoding_spec(name)?.load()?);
    cache.insert(name.to_string(), Arc::clone(&loaded));
    Ok(loaded)
}

fn get_encoding_for_model(model_name: &str) -> Result<Arc<LoadedEncoding>, String> {
    let encoding_name = resolve_encoding_name_for_model(model_name)?;
    get_encoding(encoding_name)
}

fn load_tiktoken_bpe(contents: &[u8], name: &str) -> Result<HashMap<Vec<u8>, Rank>, String> {
    let mut ranks = HashMap::new();

    for line in contents.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }

        let Some(separator_index) = line.iter().position(|b| *b == b' ') else {
            return Err(format!("invalid tiktoken BPE line in {name}"));
        };
        let token = &line[..separator_index];
        let rank = &line[separator_index + 1..];

        let token_bytes = BASE64_STANDARD
            .decode(token)
            .map_err(|err| format!("invalid base64 token in {name}: {err}"))?;
        let rank_string = std::str::from_utf8(rank)
            .map_err(|err| format!("invalid UTF-8 rank in {name}: {err}"))?;
        let rank_value = rank_string
            .parse::<u32>()
            .map_err(|err| format!("invalid rank in {name}: {err}"))?;

        ranks.insert(token_bytes, rank_value);
    }

    Ok(ranks)
}

fn build_decoder(
    mergeable_ranks: &HashMap<Vec<u8>, Rank>,
    special_tokens: &HashMap<String, Rank>,
) -> HashMap<Rank, Vec<u8>> {
    let mut decoder = mergeable_ranks
        .iter()
        .map(|(bytes, rank)| (*rank, bytes.clone()))
        .collect::<HashMap<_, _>>();

    for (token, rank) in special_tokens {
        decoder.insert(*rank, token.as_bytes().to_vec());
    }

    decoder
}

fn parse_c_string(ptr: *const c_char, field_name: &str) -> Result<String, String> {
    if ptr.is_null() {
        return Err(format!("{field_name} cannot be null"));
    }

    let value = unsafe { CStr::from_ptr(ptr) };
    value
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| format!("{field_name} must be valid UTF-8"))
}

fn parse_special_token_list(ptr: *const c_char) -> Result<Vec<String>, String> {
    if ptr.is_null() {
        return Ok(Vec::new());
    }

    let value = unsafe { CStr::from_ptr(ptr) };
    let raw = value
        .to_str()
        .map_err(|_| String::from("special token list must be valid UTF-8"))?;

    Ok(raw
        .split('\n')
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn disallowed_special_token_error(token: &str) -> String {
    format!(
        "Encountered text corresponding to disallowed special token {token:?}.\nIf you want this text to be encoded as a special token, allow it explicitly.\nIf you want this text to be encoded as normal text, use special mode EncodeAsText.\n"
    )
}

fn encode_tokens(
    encoding: &LoadedEncoding,
    text: &str,
    special_mode_raw: u32,
    special_tokens_raw: *const c_char,
) -> Result<Vec<Rank>, String> {
    let special_mode = SpecialMode::try_from(special_mode_raw)?;

    match special_mode {
        SpecialMode::Disallow => {
            if let Some(token) = first_disallowed_special(text, &encoding.special_tokens, None) {
                return Err(disallowed_special_token_error(token));
            }
            let allowed_special = HashSet::new();
            encoding
                .core_bpe
                .encode(text, &allowed_special)
                .map(|result| result.0)
                .map_err(|err| err.message)
        }
        SpecialMode::EncodeAsText => {
            let allowed_special = HashSet::new();
            encoding
                .core_bpe
                .encode(text, &allowed_special)
                .map(|result| result.0)
                .map_err(|err| err.message)
        }
        SpecialMode::AllowAll => Ok(encoding.core_bpe.encode_with_special_tokens(text)),
        SpecialMode::AllowList => {
            let allowed_tokens = parse_special_token_list(special_tokens_raw)?;
            let allowed_set = allowed_tokens.iter().cloned().collect::<HashSet<_>>();

            for token in &allowed_set {
                if !encoding.special_token_set.contains(token) {
                    return Err(format!("unknown special token for encoding: {token}"));
                }
            }

            if let Some(token) =
                first_disallowed_special(text, &encoding.special_tokens, Some(&allowed_set))
            {
                return Err(disallowed_special_token_error(token));
            }

            let allowed_refs = allowed_tokens
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            encoding
                .core_bpe
                .encode(text, &allowed_refs)
                .map(|result| result.0)
                .map_err(|err| err.message)
        }
    }
}

fn first_disallowed_special<'a>(
    text: &str,
    special_tokens: &'a [String],
    allowed: Option<&HashSet<String>>,
) -> Option<&'a str> {
    for token in special_tokens {
        if allowed.is_some_and(|allowed_set| allowed_set.contains(token)) {
            continue;
        }
        if text.contains(token) {
            return Some(token.as_str());
        }
    }
    None
}

fn decode_tokens(encoding: &LoadedEncoding, tokens: &[Rank]) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(tokens.len() * 2);
    for token in tokens {
        let token_bytes = encoding
            .decoder
            .get(token)
            .ok_or_else(|| format!("unknown token: {token}"))?;
        bytes.extend(token_bytes);
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn into_c_string_ptr(value: String) -> *mut c_char {
    CString::new(value.replace('\0', " "))
        .expect("CString creation should not fail after NUL replacement")
        .into_raw()
}

fn clear_error(out_error: *mut *mut c_char) {
    if !out_error.is_null() {
        unsafe {
            *out_error = std::ptr::null_mut();
        }
    }
}

fn write_error(out_error: *mut *mut c_char, message: String) {
    if !out_error.is_null() {
        unsafe {
            *out_error = into_c_string_ptr(message);
        }
    }
}

fn into_token_buffer(tokens: Vec<Rank>) -> TokenBuffer {
    if tokens.is_empty() {
        return TokenBuffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
    }

    let mut boxed = tokens.into_boxed_slice();
    let len = boxed.len() as u64;
    let data = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    TokenBuffer { data, len }
}

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_version() -> *mut c_char {
    into_c_string_ptr(format!(
        "tiktoken_shim/{} (openai/tiktoken {})",
        env!("CARGO_PKG_VERSION"),
        OPENAI_TIKTOKEN_VERSION
    ))
}

/// # Safety
///
/// `model_name` must be a valid NUL-terminated UTF-8 C string. `out_encoding_name`
/// must be valid to write one pointer. If `out_error` is non-null, it must be
/// valid to write one pointer. Returned strings must be released with
/// `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_encoding_name_for_model(
    model_name: *const c_char,
    out_encoding_name: *mut *mut c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_encoding_name.is_null() {
        write_error(out_error, String::from("out_encoding_name cannot be null"));
        return 1;
    }

    let result = parse_c_string(model_name, "model_name")
        .and_then(|model| resolve_encoding_name_for_model(&model).map(str::to_owned));

    match result {
        Ok(name) => {
            unsafe {
                *out_encoding_name = into_c_string_ptr(name);
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_encoding_name = std::ptr::null_mut();
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `encoding_name`, `input`, and `special_tokens` when non-null must be valid
/// NUL-terminated UTF-8 C strings. `out_count` must be valid to write one
/// `u64`. If `out_error` is non-null, it must be valid to write one pointer.
/// Returned error strings must be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_count_with_encoding(
    encoding_name: *const c_char,
    input: *const c_char,
    special_mode: u32,
    special_tokens: *const c_char,
    out_count: *mut u64,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_count.is_null() {
        write_error(out_error, String::from("out_count cannot be null"));
        return 1;
    }

    let result = parse_c_string(encoding_name, "encoding_name")
        .and_then(|name| get_encoding(&name))
        .and_then(|encoding| parse_c_string(input, "input").map(|text| (encoding, text)))
        .and_then(|(encoding, text)| encode_tokens(&encoding, &text, special_mode, special_tokens))
        .map(|tokens| tokens.len() as u64);

    match result {
        Ok(count) => {
            unsafe {
                *out_count = count;
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_count = 0;
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `model_name`, `input`, and `special_tokens` when non-null must be valid
/// NUL-terminated UTF-8 C strings. `out_count` must be valid to write one
/// `u64`. If `out_error` is non-null, it must be valid to write one pointer.
/// Returned error strings must be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_count_with_model(
    model_name: *const c_char,
    input: *const c_char,
    special_mode: u32,
    special_tokens: *const c_char,
    out_count: *mut u64,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_count.is_null() {
        write_error(out_error, String::from("out_count cannot be null"));
        return 1;
    }

    let result = parse_c_string(model_name, "model_name")
        .and_then(|name| get_encoding_for_model(&name))
        .and_then(|encoding| parse_c_string(input, "input").map(|text| (encoding, text)))
        .and_then(|(encoding, text)| encode_tokens(&encoding, &text, special_mode, special_tokens))
        .map(|tokens| tokens.len() as u64);

    match result {
        Ok(count) => {
            unsafe {
                *out_count = count;
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_count = 0;
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `encoding_name`, `input`, and `special_tokens` when non-null must be valid
/// NUL-terminated UTF-8 C strings. `out_tokens` must be valid to write one
/// `TokenBuffer`. If `out_error` is non-null, it must be valid to write one
/// pointer. Returned buffers must be released with `tiktoken_free_u32_buffer`,
/// and returned error strings must be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_encode_with_encoding(
    encoding_name: *const c_char,
    input: *const c_char,
    special_mode: u32,
    special_tokens: *const c_char,
    out_tokens: *mut TokenBuffer,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_tokens.is_null() {
        write_error(out_error, String::from("out_tokens cannot be null"));
        return 1;
    }

    let result = parse_c_string(encoding_name, "encoding_name")
        .and_then(|name| get_encoding(&name))
        .and_then(|encoding| parse_c_string(input, "input").map(|text| (encoding, text)))
        .and_then(|(encoding, text)| encode_tokens(&encoding, &text, special_mode, special_tokens));

    match result {
        Ok(tokens) => {
            unsafe {
                *out_tokens = into_token_buffer(tokens);
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_tokens = TokenBuffer {
                    data: std::ptr::null_mut(),
                    len: 0,
                };
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `model_name`, `input`, and `special_tokens` when non-null must be valid
/// NUL-terminated UTF-8 C strings. `out_tokens` must be valid to write one
/// `TokenBuffer`. If `out_error` is non-null, it must be valid to write one
/// pointer. Returned buffers must be released with `tiktoken_free_u32_buffer`,
/// and returned error strings must be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_encode_with_model(
    model_name: *const c_char,
    input: *const c_char,
    special_mode: u32,
    special_tokens: *const c_char,
    out_tokens: *mut TokenBuffer,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_tokens.is_null() {
        write_error(out_error, String::from("out_tokens cannot be null"));
        return 1;
    }

    let result = parse_c_string(model_name, "model_name")
        .and_then(|name| get_encoding_for_model(&name))
        .and_then(|encoding| parse_c_string(input, "input").map(|text| (encoding, text)))
        .and_then(|(encoding, text)| encode_tokens(&encoding, &text, special_mode, special_tokens));

    match result {
        Ok(tokens) => {
            unsafe {
                *out_tokens = into_token_buffer(tokens);
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_tokens = TokenBuffer {
                    data: std::ptr::null_mut(),
                    len: 0,
                };
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `encoding_name` must be a valid NUL-terminated UTF-8 C string. `tokens`
/// must be null when `len` is zero, or valid to read `len` `u32` values.
/// `out_text` must be valid to write one pointer. If `out_error` is non-null,
/// it must be valid to write one pointer. Returned strings and returned error
/// strings must be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_decode_with_encoding(
    encoding_name: *const c_char,
    tokens: *const u32,
    len: u64,
    out_text: *mut *mut c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_text.is_null() {
        write_error(out_error, String::from("out_text cannot be null"));
        return 1;
    }
    if tokens.is_null() && len != 0 {
        unsafe {
            *out_text = std::ptr::null_mut();
        }
        write_error(
            out_error,
            String::from("tokens cannot be null when len is non-zero"),
        );
        return 1;
    }

    let result = parse_c_string(encoding_name, "encoding_name")
        .and_then(|name| get_encoding(&name))
        .and_then(|encoding| {
            let token_slice = if len == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(tokens, len as usize) }
            };
            decode_tokens(&encoding, token_slice)
        });

    match result {
        Ok(text) => {
            unsafe {
                *out_text = into_c_string_ptr(text);
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_text = std::ptr::null_mut();
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `model_name` must be a valid NUL-terminated UTF-8 C string. `tokens` must
/// be null when `len` is zero, or valid to read `len` `u32` values. `out_text`
/// must be valid to write one pointer. If `out_error` is non-null, it must be
/// valid to write one pointer. Returned strings and returned error strings must
/// be released with `tiktoken_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_decode_with_model(
    model_name: *const c_char,
    tokens: *const u32,
    len: u64,
    out_text: *mut *mut c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    clear_error(out_error);

    if out_text.is_null() {
        write_error(out_error, String::from("out_text cannot be null"));
        return 1;
    }
    if tokens.is_null() && len != 0 {
        unsafe {
            *out_text = std::ptr::null_mut();
        }
        write_error(
            out_error,
            String::from("tokens cannot be null when len is non-zero"),
        );
        return 1;
    }

    let result = parse_c_string(model_name, "model_name")
        .and_then(|name| get_encoding_for_model(&name))
        .and_then(|encoding| {
            let token_slice = if len == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(tokens, len as usize) }
            };
            decode_tokens(&encoding, token_slice)
        });

    match result {
        Ok(text) => {
            unsafe {
                *out_text = into_c_string_ptr(text);
            }
            0
        }
        Err(err) => {
            unsafe {
                *out_text = std::ptr::null_mut();
            }
            write_error(out_error, err);
            1
        }
    }
}

/// # Safety
///
/// `ptr` must be null or a pointer returned by this library from
/// `tiktoken_version`, `tiktoken_encoding_name_for_model`,
/// `tiktoken_decode_with_encoding`, `tiktoken_decode_with_model`, or an error output.
/// It must be released at most once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// # Safety
///
/// `ptr` and `len` must be the exact buffer pair returned by
/// `tiktoken_encode_with_encoding` or `tiktoken_encode_with_model`. The buffer
/// must be released at most once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tiktoken_free_u32_buffer(ptr: *mut u32, len: u64) {
    if !ptr.is_null() {
        unsafe {
            let slice = std::ptr::slice_from_raw_parts_mut(ptr, len as usize);
            drop(Box::from_raw(slice));
        }
    }
}
