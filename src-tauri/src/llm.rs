//! The model call. Lives in Rust so the API key never enters the webview —
//! which matters more here than in a normal app, because the webview is
//! running code the model itself wrote.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";
pub const MODELS: [&str; 2] = ["claude-sonnet-5", "claude-opus-5"];

const KEYCHAIN_SERVICE: &str = "com.claya.app";
/// Pre-rename service name. macOS derives nothing about Keychain items from the
/// bundle id automatically, so renaming stranded the key the user had already
/// stored. Read it as a fallback and promote it forward on first use, so this
/// constant can be deleted once no install is still carrying an old item.
const LEGACY_KEYCHAIN_SERVICES: [&str; 1] = ["com.selfbuild.dev"];
const KEYCHAIN_USER: &str = "anthropic-api-key";

/// The rules a generated module must satisfy. Kept in lockstep with
/// `src/canvas/loader.ts` — if you widen the registry, widen this too.
const CONTRACT: &str = r#"You are the engine behind a self-modifying macOS app. The user describes a change; you rewrite the app's single React module to make it true.

HARD CONTRACT — a module violating any of these is rejected before it runs:

1. Default-export a React component function.
2. The ONLY importable modules are "react", "@host" and "@ui". Any other import
   is rejected at compile time. Dynamic import() is rejected.
3. Persist exclusively through the host API:
       import { host } from "@host";
       host.state.get<T>(key: string, fallback: T): Promise<T>
       host.state.set(key: string, value: unknown): Promise<void>
   There is no fetch, no localStorage, no filesystem. Values are JSON.
4. TypeScript + JSX, compiled by Sucrase at runtime with no type checker.
   Avoid anything needing type information to erase (`const enum`,
   `namespace`, legacy decorators). Plain types, interfaces and generics are fine.
5. Build the interface from "@ui" — Radix Themes. Reach for a component before
   you hand-roll one; only use inline `style={{ ... }}` for spacing or layout a
   component cannot express. You cannot add a stylesheet.

   Available (this list is exhaustive — anything else fails):

       Layout      Box Flex Grid Container Section Inset Separator ScrollArea
       Typography  Heading Text Link Code Em Strong Quote Blockquote Kbd
       Controls    Button IconButton TextField TextArea Checkbox CheckboxGroup
                   CheckboxCards Switch Select RadioGroup RadioCards Radio
                   Slider SegmentedControl
       Display     Card Badge Avatar Table DataList Callout Progress Spinner
                   Skeleton AspectRatio Separator
       Overlay     Dialog AlertDialog Popover HoverCard Tooltip DropdownMenu
                   ContextMenu
       Navigation  Tabs TabNav

   Composite parts are namespaced, e.g. `<Card>`, `<Tabs.Root>`, `<Tabs.List>`,
   `<Tabs.Trigger>`, `<Tabs.Content>`, `<Select.Root>`, `<Select.Trigger>`,
   `<Select.Content>`, `<Select.Item>`, `<Dialog.Root>`, `<Dialog.Trigger>`,
   `<Dialog.Content>`, `<Dialog.Title>`, `<TextField.Root>`.

   DO NOT render `<Theme>` yourself. The app wraps you in one already, and its
   colour, radius and appearance are the user's choice — not yours.

   NEVER hard-code a colour. Not a hex value, and not a named one either —
   `color: "white"` is exactly as wrong as a hex value like #fff, because both make
   that element immune to the user's theme. Instead:

     - text: `<Text color="gray">`, `<Text highContrast>`, or no colour at all,
       which inherits correctly on light and dark
     - emphasis: `color` / `variant` props on Button, Badge, Card, IconButton
     - surfaces: `<Card>`, or `<Box>` with a Radix background — not a literal

   If the user asks for one specific colour ("make the background black"), set
   that ONE value and leave everything else to the theme. Do not then hard-code
   foreground colours to compensate: pick components and variants that already
   read correctly on a dark surface.
6. Declare the data you own, as a named export alongside the component:

       export const schema = {
         todos: { type: "record-list", key: "id", fields: ["id", "title", "done"] },
       };

   Shapes: `{ type: "scalar" }` for an opaque value, `{ type: "record", fields: [...] }`
   for one object, `{ type: "record-list", key, fields: [...] }` for an array of
   objects identified by `key`.

   THE RULE: you may only destroy fields you declare. A field you do not list is
   preserved when you write, because it belongs to a version other than yours.
   So list every field YOUR version reads or writes, and nothing else. Versions of
   this app before and after yours may store extra fields on the same records —
   leaving them out of `fields` is what keeps them alive.

OUTPUT RULES:
- Always call the `edit_module` tool. Never reply with prose alone.
- `source` must be the COMPLETE new file. Never a diff, never an excerpt,
  never "// ... rest unchanged". The string you return replaces the file.
- Preserve existing behaviour and data unless the user asked you to change it.
  Data already saved under a host.state key stays readable — if you change a
  key's shape, migrate it in code rather than orphaning it.
- Keep it one self-contained file. Helper components go in the same file."#;

#[derive(Serialize, Clone)]
pub struct Proposal {
    pub source: String,
    pub note: String,
    pub explanation: String,
}

#[derive(Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
}

/// Environment first, Keychain second.
///
/// The env var exists for development. macOS binds a Keychain item's ACL to the
/// calling binary's code signature, and `cargo build` produces a new one on every
/// build — so a dev binary is a different app to the Keychain each time, and gets
/// re-prompted. `export ANTHROPIC_API_KEY=...` sidesteps that entirely. A signed
/// release build has a stable identity and is authorised once.
/// Read once per process, not once per request.
///
/// macOS authorises a Keychain item against the *reading binary's* code
/// signature, and `cargo build` emits a new one every build — so in development
/// each rebuild is a fresh app to the Keychain and prompts again. Caching cannot
/// fix that across rebuilds (nothing can, short of a stable signing identity),
/// but it does stop a long session prompting once per message.
static CACHED_KEY: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

pub fn get_key() -> Result<String, String> {
    if let Ok(from_env) = std::env::var("ANTHROPIC_API_KEY") {
        let trimmed = from_env.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(guard) = CACHED_KEY.read() {
        if let Some(key) = guard.as_deref() {
            return Ok(key.to_string());
        }
    }

    let key = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
        .or_else(|| {
            // Fall back to a pre-rename item, then write it under the current
            // name so the next launch finds it directly and this path goes cold.
            LEGACY_KEYCHAIN_SERVICES.iter().find_map(|service| {
                let found = keyring::Entry::new(service, KEYCHAIN_USER)
                    .ok()?
                    .get_password()
                    .ok()?;
                if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
                    let _ = entry.set_password(&found);
                }
                println!("[key] adopted from legacy keychain item \"{service}\"");
                Some(found)
            })
        })
        .ok_or_else(|| "No API key set. Add one in the shell window.".to_string())?;

    if let Ok(mut guard) = CACHED_KEY.write() {
        *guard = Some(key.clone());
    }
    Ok(key)
}

fn invalidate_cache() {
    if let Ok(mut guard) = CACHED_KEY.write() {
        *guard = None;
    }
}

/// Move a pre-rename key onto the current service name.
///
/// Run once at startup when no key is recorded, because the shell's key gate
/// blocks sending — so the lazy fallback inside `get_key` would never get a
/// chance to fire. Reading a Keychain item that does not exist returns NoEntry
/// without prompting, so this is silent for anyone who never had an old key.
pub fn adopt_legacy_key() -> bool {
    if keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
        .is_some()
    {
        return true;
    }
    LEGACY_KEYCHAIN_SERVICES.iter().any(|service| {
        let Some(found) = keyring::Entry::new(service, KEYCHAIN_USER)
            .ok()
            .and_then(|e| e.get_password().ok())
        else {
            return false;
        };
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
            let _ = entry.set_password(&found);
        }
        println!("[key] adopted from legacy keychain item \"{service}\"");
        true
    })
}

pub fn env_key_present() -> bool {
    std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

pub fn set_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("key is empty".into());
    }
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|e| format!("keychain unavailable: {e}"))?
        .set_password(key)
        .map_err(|e| format!("could not store key: {e}"))?;
    invalidate_cache();
    Ok(())
}

pub fn clear_key() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|e| format!("keychain unavailable: {e}"))?;
    invalidate_cache();
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("could not clear key: {e}")),
    }
}

fn tool_definition() -> Value {
    json!({
        "name": "edit_module",
        "description": "Replace the app module with a complete new source file.",
        "strict": true,
        "input_schema": {
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "The complete new TSX source for the module. Not a diff."
                },
                "note": {
                    "type": "string",
                    "description": "Imperative summary under 60 chars for the version history, e.g. 'add due dates'."
                },
                "explanation": {
                    "type": "string",
                    "description": "One or two sentences telling the user what changed and why."
                }
            },
            "required": ["source", "note", "explanation"],
            "additionalProperties": false
        }
    })
}

/// Prior turns are replayed as prompt + explanation only — never past sources.
/// Full history of a file this size would dominate the context budget within a
/// handful of edits, and the current source already carries that information.
fn build_messages(history: &[Turn], source: &str, prompt: &str) -> Vec<Value> {
    // Gate rollbacks are stored as "system" so the shell can style them apart,
    // but the Messages API only takes user/assistant here — Sonnet does not
    // accept a mid-conversation system role, and the model should see these
    // anyway: they are what happened after its last attempt.
    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| {
            let role = if t.role == "user" { "user" } else { "assistant" };
            json!({ "role": role, "content": t.content })
        })
        .collect();

    messages.push(json!({
        "role": "user",
        "content": format!(
            "Current module source:\n\n```tsx\n{source}\n```\n\nRequested change: {prompt}"
        )
    }));
    messages
}

pub async fn propose(
    key: &str,
    model: &str,
    source: &str,
    history: &[Turn],
    prompt: &str,
) -> Result<Proposal, String> {
    let body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": CONTRACT,
        "tools": [tool_definition()],
        "tool_choice": { "type": "tool", "name": "edit_module" },
        "messages": build_messages(history, source, prompt),
    });

    let response = reqwest::Client::new()
        .post(ENDPOINT)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|e| format!("could not read response: {e}"))?;

    if !status.is_success() {
        let detail = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("API {status}: {detail}"));
    }

    if payload.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        let category = payload
            .pointer("/stop_details/category")
            .and_then(Value::as_str)
            .unwrap_or("unspecified");
        return Err(format!("The model declined this request ({category})."));
    }

    let block = payload
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
            blocks.iter().find(|b| {
                b.get("type").and_then(Value::as_str) == Some("tool_use")
                    && b.get("name").and_then(Value::as_str) == Some("edit_module")
            })
        })
        .ok_or_else(|| "model returned no edit_module call".to_string())?;

    let input = block
        .get("input")
        .ok_or_else(|| "edit_module call had no input".to_string())?;

    let field = |name: &str| -> Result<String, String> {
        input
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("edit_module call was missing \"{name}\""))
    };

    Ok(Proposal {
        source: field("source")?,
        note: field("note")?,
        explanation: field("explanation")?,
    })
}
