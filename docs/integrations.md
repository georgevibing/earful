# Integrating clients

Any client that can make HTTPS requests and compute SHA-256 can submit feedback. The full contract is in [api.md](api.md); this page is the practical guide.

For web pages, the easiest options are the hosted page (`https://<host>/s/<slug>`) or the embeddable widget ([packages/widget/README.md](../packages/widget/README.md)), which implement everything below for you.

## Protocol

1. **Fetch a challenge** early: `GET /api/v1/challenge?key=<ingest_key>` returns `{ "challenge", "bits", "expires_in" }`.
2. **Solve it**: find a decimal `nonce` such that `sha256(utf8(challenge + ":" + nonce))` starts with at least `bits` zero bits. Counting up from `0` is the reference strategy. At the default 18 bits this is about 260k hashes on average: a fraction of a second in native code, around a second in slow scripting runtimes. Solve off the main thread or in small slices per frame.
3. **Submit**: `POST /api/v1/feedback` with JSON:

   ```json
   {
     "key": "ear_...",
     "kind": "bug",
     "title": "Crash when opening inventory",
     "body": "Pressed I during the boss fight, game closed.",
     "contact": "optional@example.com",
     "meta": { "version": "1.4.2", "platform": "windows", "build": "8812" },
     "challenge": "<challenge>",
     "nonce": "<nonce>"
   }
   ```

   `kind` is `bug` or `feedback`. `title`, `contact`, `rating` (1 to 5) and `meta` (JSON object, max 4 KB) are optional; `body` needs 3 to 10000 characters.
4. **Success is `202 {"ok": true}`**. Nothing else to read.

Each challenge works once and is bound to its project. Submissions sent less than `EARFUL_MIN_FILL_SECS` (default 2 seconds) after the challenge was issued are quarantined as `too_fast`, so fetch the challenge when the form opens, not when the user presses send.

### Error handling

| Response | Meaning | What the client should do |
| --- | --- | --- |
| `400 invalid_<field>` | Validation failed (`message` says why). | Show the message, let the user fix the field. |
| `400 challenge_expired`, `challenge_used`, `challenge_invalid`, `challenge_malformed`, `challenge_wrong_project`, `pow_insufficient` | The proof of work is not acceptable. | Fetch a new challenge, solve, retry once. |
| `403 project_paused` | The project is not accepting feedback. | Tell the user, do not retry. |
| `404 not_found` | Unknown ingest key. | Check the configured key (it may have been rotated). |
| `429 rate_limited` | Too many submissions. | Wait `Retry-After` seconds; do not loop. |
| `5xx` / network error | Server trouble. | Keep the report locally and retry later with a fresh challenge. |

### Advice for games and apps

- **The ingest key is not a secret.** It ships inside your binary or page and anyone can extract it. That is fine: it only identifies the project. If it gets abused, rotate it in the admin UI (old builds then get `404`) or pause the project. Never put any real credential into a client.
- **Fetch the challenge when the feedback dialog opens** and solve it while the player types. By the time they press send, the work is done and the minimum fill time has passed.
- **Put build info in `meta`**: version, build number, platform, OS, GPU, current level or scene. It shows up pretty-printed in the admin UI.
- **Only collect contact details the user typed in.** Do not attach account ids or hardware ids silently.
- **Automatic crash reports**: save the report to disk on crash and send it on next launch (fetch challenge, solve, wait 2 seconds, submit). Identical reports from different players are merged into one row with a duplicate counter, so you see "+37" instead of 37 rows.

## curl

```sh
HOST=https://feedback.example.com
KEY=ear_your_ingest_key

CH=$(curl -fsS "$HOST/api/v1/challenge?key=$KEY")
CHALLENGE=$(echo "$CH" | jq -r .challenge)
BITS=$(echo "$CH" | jq -r .bits)

# Reference solver built into the server binary (or: docker run --rm earful:local solve ...)
NONCE=$(earful solve "$CHALLENGE" "$BITS")
sleep 2   # minimum fill time

jq -n --arg key "$KEY" --arg challenge "$CHALLENGE" --arg nonce "$NONCE" '{
  key: $key, kind: "bug",
  title: "Test from curl", body: "Hello from the command line",
  meta: { platform: "cli" },
  challenge: $challenge, nonce: $nonce
}' | curl -fsS -X POST "$HOST/api/v1/feedback" -H 'content-type: application/json' --data @-
```

## Unity (C#)

```csharp
using System;
using System.Collections;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using UnityEngine;
using UnityEngine.Networking;

public class FeedbackClient : MonoBehaviour
{
    const string Endpoint = "https://feedback.example.com";
    const string IngestKey = "ear_your_ingest_key";

    [Serializable] class ChallengeResponse { public string challenge; public int bits; public int expires_in; }
    [Serializable] class Meta { public string version; public string platform; }
    [Serializable] class Submission
    {
        public string key, kind, title, body, contact, challenge, nonce;
        public Meta meta;
    }

    string challenge;
    Task<string> solving;

    // Call when the feedback dialog opens.
    public IEnumerator Prepare()
    {
        using var req = UnityWebRequest.Get($"{Endpoint}/api/v1/challenge?key={Uri.EscapeDataString(IngestKey)}");
        yield return req.SendWebRequest();
        if (req.result != UnityWebRequest.Result.Success) { Debug.LogWarning(req.error); yield break; }
        var ch = JsonUtility.FromJson<ChallengeResponse>(req.downloadHandler.text);
        challenge = ch.challenge;
        // Background thread so the frame rate does not suffer. (WebGL has no threads: solve in a coroutine instead.)
        solving = Task.Run(() => Solve(ch.challenge, ch.bits));
    }

    public IEnumerator Submit(string kind, string title, string body, string contact, Action<bool> done)
    {
        if (solving == null) { done(false); yield break; }
        while (!solving.IsCompleted) yield return null;

        var json = JsonUtility.ToJson(new Submission
        {
            key = IngestKey, kind = kind, title = title, body = body, contact = contact,
            meta = new Meta { version = Application.version, platform = Application.platform.ToString() },
            challenge = challenge, nonce = solving.Result,
        });
        using var req = new UnityWebRequest($"{Endpoint}/api/v1/feedback", "POST");
        req.uploadHandler = new UploadHandlerRaw(Encoding.UTF8.GetBytes(json));
        req.downloadHandler = new DownloadHandlerBuffer();
        req.SetRequestHeader("Content-Type", "application/json");
        yield return req.SendWebRequest();
        solving = null; // challenges are single use
        done(req.responseCode == 202);
    }

    static string Solve(string challenge, int bits)
    {
        using var sha = SHA256.Create();
        for (ulong n = 0; ; n++)
        {
            var nonce = n.ToString();
            var hash = sha.ComputeHash(Encoding.UTF8.GetBytes(challenge + ":" + nonce));
            if (LeadingZeroBits(hash) >= bits) return nonce;
        }
    }

    static int LeadingZeroBits(byte[] hash)
    {
        int bits = 0;
        foreach (var b in hash)
        {
            if (b == 0) { bits += 8; continue; }
            for (int i = 7; i >= 0 && ((b >> i) & 1) == 0; i--) bits++;
            break;
        }
        return bits;
    }
}
```

`JsonUtility` cannot omit fields, so leave `rating` out of the class unless you always send a value from 1 to 5.

## Godot 4 (GDScript)

```gdscript
extends Node

const ENDPOINT := "https://feedback.example.com"
const INGEST_KEY := "ear_your_ingest_key"

var _challenge := ""
var _nonce := ""

# Call when the feedback dialog opens.
func prepare() -> void:
	var http := HTTPRequest.new()
	add_child(http)
	http.request(ENDPOINT + "/api/v1/challenge?key=" + INGEST_KEY.uri_encode())
	var res: Array = await http.request_completed  # [result, code, headers, body]
	http.queue_free()
	if res[1] != 200:
		push_warning("feedback challenge failed: %d" % res[1])
		return
	var data: Dictionary = JSON.parse_string(res[3].get_string_from_utf8())
	_challenge = data["challenge"]
	_nonce = await _solve(_challenge, int(data["bits"]))

func submit(kind: String, title: String, body: String, contact := "") -> bool:
	if _nonce.is_empty():
		return false
	var payload := {
		"key": INGEST_KEY, "kind": kind, "title": title, "body": body, "contact": contact,
		"meta": {
			"version": str(ProjectSettings.get_setting("application/config/version", "")),
			"platform": OS.get_name(),
		},
		"challenge": _challenge, "nonce": _nonce,
	}
	var http := HTTPRequest.new()
	add_child(http)
	http.request(ENDPOINT + "/api/v1/feedback", ["Content-Type: application/json"],
		HTTPClient.METHOD_POST, JSON.stringify(payload))
	var res: Array = await http.request_completed
	http.queue_free()
	_challenge = ""  # single use
	_nonce = ""
	return res[1] == 202

func _solve(challenge: String, bits: int) -> String:
	var ctx := HashingContext.new()
	var n := 0
	while true:
		ctx.start(HashingContext.HASH_SHA256)
		ctx.update(("%s:%d" % [challenge, n]).to_utf8_buffer())
		if _leading_zero_bits(ctx.finish()) >= bits:
			return str(n)
		n += 1
		if n % 2000 == 0:
			await get_tree().process_frame  # spread the work over frames
	return ""

func _leading_zero_bits(digest: PackedByteArray) -> int:
	var bits := 0
	for b in digest:
		if b == 0:
			bits += 8
			continue
		var mask := 0x80
		while mask > 0 and (b & mask) == 0:
			bits += 1
			mask >>= 1
		break
	return bits
```

GDScript hashing is slower than native code, so solving can take a second or two at 18 bits. Starting in `prepare()` hides that behind the typing time. Alternatively move `_solve` into a `Thread`.

## C++ (sketch)

Any SHA-256 implementation works: OpenSSL's `SHA256()`, a platform API, or a header-only library such as [picosha2](https://github.com/okdshin/PicoSHA2). HTTP via libcurl, cpr, or your engine's HTTP module.

```cpp
#include <array>
#include <string>
#include "picosha2.h"

int leading_zero_bits(const std::array<unsigned char, 32>& h) {
    int bits = 0;
    for (unsigned char b : h) {
        if (b == 0) { bits += 8; continue; }
        for (int i = 7; i >= 0 && !((b >> i) & 1); --i) ++bits;
        break;
    }
    return bits;
}

std::string solve(const std::string& challenge, int bits) {
    std::array<unsigned char, 32> hash{};
    for (unsigned long long n = 0;; ++n) {
        const std::string input = challenge + ":" + std::to_string(n);
        picosha2::hash256(input.begin(), input.end(), hash.begin(), hash.end());
        if (leading_zero_bits(hash) >= bits) return std::to_string(n);
    }
}

// 1. GET  {endpoint}/api/v1/challenge?key={key}            -> parse "challenge" and "bits"
// 2. std::async(std::launch::async, solve, challenge, bits)  while the dialog is open
// 3. POST {endpoint}/api/v1/feedback  Content-Type: application/json
//    {"key":..., "kind":"bug", "title":..., "body":..., "meta":{...}, "challenge":..., "nonce":...}
// 4. 202 means done. Escape strings properly when building the JSON (use a JSON library).
```

## Python

```python
import hashlib
import json
import time
import urllib.parse
import urllib.request

ENDPOINT = "https://feedback.example.com"
KEY = "ear_your_ingest_key"


def leading_zero_bits(digest: bytes) -> int:
    bits = 0
    for b in digest:
        if b == 0:
            bits += 8
            continue
        return bits + (8 - b.bit_length())
    return bits


def solve(challenge: str, bits: int) -> str:
    n = 0
    while leading_zero_bits(hashlib.sha256(f"{challenge}:{n}".encode()).digest()) < bits:
        n += 1
    return str(n)


def send(kind: str, title: str, body: str, meta: dict | None = None) -> bool:
    url = f"{ENDPOINT}/api/v1/challenge?key={urllib.parse.quote(KEY)}"
    with urllib.request.urlopen(url) as resp:
        ch = json.load(resp)
    started = time.monotonic()
    nonce = solve(ch["challenge"], ch["bits"])
    time.sleep(max(0.0, 2.0 - (time.monotonic() - started)))  # minimum fill time

    payload = {
        "key": KEY, "kind": kind, "title": title, "body": body, "meta": meta or {},
        "challenge": ch["challenge"], "nonce": nonce,
    }
    req = urllib.request.Request(
        f"{ENDPOINT}/api/v1/feedback",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:  # raises HTTPError on 4xx/5xx
        return resp.status == 202


if __name__ == "__main__":
    print(send("feedback", "Hello", "Sent from Python", {"platform": "python"}))
```


## Diagnostic reports

The existing POST /api/v1/feedback and SDK submit accept kind crash, exception and
log alongside bug and feedback. MCP create_feedback accepts the same kinds and
diagnostics. An optional diagnostics JSON object holds at most 65536 serialized
UTF-8 bytes; omitted/null becomes {}. Other types or larger objects return
invalid_diagnostics. The public request limit is 128 KiB; existing body (3-10000
characters), meta (4096 bytes), PoW and rate limits still apply.

Diagnostics round-trip through list/detail, MCP and JSON/CSV exports. Filter by
kind with the existing status/project/label filters. Stats include kind_counts
excluding spam. The daily chart still shows bugs and feedback only. Search indexes
title/body, so include the exception signature there. See docs/crash-reporting.md
in the repository for integration and agent workflows.
