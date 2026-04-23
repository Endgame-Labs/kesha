use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::blocking::Client;
use sha1::Digest as Sha1Digest;
use sha1::Sha1;
use sha2::Sha256;
use tiktoken::{CoreBPE, Rank};

const OPENAI_TIKTOKEN_VERSION: &str = "0.12.0";
const R50K_PAT_STR: &str =
    "'(?:[sdmt]|ll|ve|re)| ?\\p{L}++| ?\\p{N}++| ?[^\\s\\p{L}\\p{N}]++|\\s++$|\\s+(?!\\S)|\\s";
const CL100K_PAT_STR: &str = "'(?i:[sdmt]|ll|ve|re)|[^\\r\\n\\p{L}\\p{N}]?+\\p{L}++|\\p{N}{1,3}+| ?[^\\s\\p{L}\\p{N}]++[\\r\\n]*+|\\s++$|\\s*[\\r\\n]|\\s+(?!\\S)|\\s";
const O200K_PAT_STR: &str = "[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]*[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?|[^\\r\\n\\p{L}\\p{N}]?[\\p{Lu}\\p{Lt}\\p{Lm}\\p{Lo}\\p{M}]+[\\p{Ll}\\p{Lm}\\p{Lo}\\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n/]*|\\s*[\\r\\n]+|\\s+(?!\\S)|\\s+";

static ENCODING_CACHE: LazyLock<Mutex<HashMap<String, Arc<LoadedEncoding>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .build()
        .expect("failed to build HTTP client")
});

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
    special_tokens: Vec<String>,
    special_token_set: HashSet<String>,
}

struct EncodingSpec {
    bpe_url: &'static str,
    expected_hash: &'static str,
    pat_str: &'static str,
    special_tokens: fn() -> HashMap<String, Rank>,
}

impl EncodingSpec {
    fn load(&self) -> Result<LoadedEncoding, String> {
        let mergeable_ranks = load_tiktoken_bpe(self.bpe_url, self.expected_hash)?;
        let special_tokens = (self.special_tokens)();

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
            special_tokens: special_token_names,
            special_token_set,
        })
    }
}

fn encoding_spec(name: &str) -> Result<EncodingSpec, String> {
    match name {
        "gpt2" | "r50k_base" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/r50k_base.tiktoken",
            expected_hash: "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930",
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_r50k_base,
        }),
        "p50k_base" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/p50k_base.tiktoken",
            expected_hash: "94b5ca7dff4d00767bc256fdd1b27e5b17361d7b8a5f968547f9f23eb70d2069",
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_p50k_base,
        }),
        "p50k_edit" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/p50k_base.tiktoken",
            expected_hash: "94b5ca7dff4d00767bc256fdd1b27e5b17361d7b8a5f968547f9f23eb70d2069",
            pat_str: R50K_PAT_STR,
            special_tokens: special_tokens_p50k_edit,
        }),
        "cl100k_base" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/cl100k_base.tiktoken",
            expected_hash: "223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7",
            pat_str: CL100K_PAT_STR,
            special_tokens: special_tokens_cl100k_base,
        }),
        "o200k_base" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/o200k_base.tiktoken",
            expected_hash: "446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d",
            pat_str: O200K_PAT_STR,
            special_tokens: special_tokens_o200k_base,
        }),
        "o200k_harmony" => Ok(EncodingSpec {
            bpe_url: "https://openaipublic.blob.core.windows.net/encodings/o200k_base.tiktoken",
            expected_hash: "446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d",
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

fn load_tiktoken_bpe(path: &str, expected_hash: &str) -> Result<HashMap<Vec<u8>, Rank>, String> {
    let contents = read_file_cached(path, expected_hash)?;
    let mut ranks = HashMap::new();

    for line in contents.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }

        let Some(separator_index) = line.iter().position(|b| *b == b' ') else {
            return Err(format!("invalid tiktoken BPE line in {path}"));
        };
        let token = &line[..separator_index];
        let rank = &line[separator_index + 1..];

        let token_bytes = BASE64_STANDARD
            .decode(token)
            .map_err(|err| format!("invalid base64 token in {path}: {err}"))?;
        let rank_string = std::str::from_utf8(rank)
            .map_err(|err| format!("invalid UTF-8 rank in {path}: {err}"))?;
        let rank_value = rank_string
            .parse::<u32>()
            .map_err(|err| format!("invalid rank in {path}: {err}"))?;

        ranks.insert(token_bytes, rank_value);
    }

    Ok(ranks)
}

fn read_file_cached(path: &str, expected_hash: &str) -> Result<Vec<u8>, String> {
    let (cache_dir, user_specified_cache) = cache_directory();
    if let Some(cache_dir) = &cache_dir {
        let cache_key = format!("{:x}", Sha1::digest(path.as_bytes()));
        let cache_path = cache_dir.join(cache_key);
        if cache_path.exists() {
            let data = fs::read(&cache_path).map_err(|err| {
                format!("failed to read cache file {}: {err}", cache_path.display())
            })?;
            if sha256_hex(&data) == expected_hash {
                return Ok(data);
            }
            let _ = fs::remove_file(cache_path);
        }
    }

    let contents = read_file(path)?;
    if sha256_hex(&contents) != expected_hash {
        return Err(format!(
            "hash mismatch for data downloaded from {path} (expected {expected_hash})"
        ));
    }

    if let Some(cache_dir) = cache_dir {
        let cache_key = format!("{:x}", Sha1::digest(path.as_bytes()));
        let cache_path = cache_dir.join(cache_key);
        if let Err(err) = write_cache_file(&cache_dir, &cache_path, &contents) {
            if user_specified_cache {
                return Err(err);
            }
        }
    }

    Ok(contents)
}

fn cache_directory() -> (Option<PathBuf>, bool) {
    if let Ok(dir) = env::var("TIKTOKEN_CACHE_DIR") {
        if dir.is_empty() {
            return (None, true);
        }
        return (Some(PathBuf::from(dir)), true);
    }
    if let Ok(dir) = env::var("DATA_GYM_CACHE_DIR") {
        if dir.is_empty() {
            return (None, true);
        }
        return (Some(PathBuf::from(dir)), true);
    }
    (Some(env::temp_dir().join("data-gym-cache")), false)
}

fn write_cache_file(cache_dir: &Path, cache_path: &Path, contents: &[u8]) -> Result<(), String> {
    fs::create_dir_all(cache_dir).map_err(|err| {
        format!(
            "failed to create cache directory {}: {err}",
            cache_dir.display()
        )
    })?;

    let tmp_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("failed to get system time: {err}"))?
        .as_nanos();
    let tmp_path = cache_path.with_extension(format!("{tmp_suffix}.tmp"));

    fs::write(&tmp_path, contents).map_err(|err| {
        format!(
            "failed to write cache temp file {}: {err}",
            tmp_path.display()
        )
    })?;
    fs::rename(&tmp_path, cache_path).map_err(|err| {
        format!(
            "failed to move cache temp file {} into place {}: {err}",
            tmp_path.display(),
            cache_path.display()
        )
    })?;
    Ok(())
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    if !path.contains("://") {
        return fs::read(path).map_err(|err| format!("failed to read {path}: {err}"));
    }

    if !path.starts_with("http://") && !path.starts_with("https://") {
        return Err(format!("unsupported URI scheme in {path}"));
    }

    let response = HTTP_CLIENT
        .get(path)
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|err| format!("failed to download {path}: {err}"))?;

    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|err| format!("failed to read response body from {path}: {err}"))
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_encoding_name_for_model(
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_count_with_encoding(
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_count_with_model(
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_encode_with_encoding(
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_encode_with_model(
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

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tiktoken_free_u32_buffer(ptr: *mut u32, len: u64) {
    if !ptr.is_null() {
        unsafe {
            let slice = std::ptr::slice_from_raw_parts_mut(ptr, len as usize);
            drop(Box::from_raw(slice));
        }
    }
}
