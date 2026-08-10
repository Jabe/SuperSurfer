// Matcher helpers and config utilities injected before user config.

function host(hostname) {
  return (url) => url.hostname === hostname;
}

function domain(name) {
  return (url) => globalThis.__domainMatch(url.hostname, name);
}

function suffix(s) {
  const bare = s.startsWith(".") ? s.slice(1) : s;
  // Require a label boundary so suffix("example.com") does not match notexample.com.
  return (url) => url.hostname === bare || url.hostname.endsWith("." + bare);
}

function glob(pattern) {
  return (url) => {
    const target = url.hostname + url.pathname;
    return globalThis.__globMatch(pattern, target);
  };
}

function path(pattern) {
  return (url) => globalThis.__pathMatch(pattern, url.pathname);
}

function regex(re) {
  return (url) => re.test(url.href);
}

function all(...matchers) {
  return (url, ctx) => matchers.every((m) => __evalMatch(m, url, ctx));
}

function not(matcher) {
  return (url, ctx) => !__evalMatch(matcher, url, ctx);
}

function processRunning(name) {
  return !!globalThis.__processRunning(String(name));
}

function __evalMatch(matcher, url, ctx) {
  if (typeof matcher === "function") {
    return !!matcher(url, ctx);
  }
  if (Array.isArray(matcher)) {
    return matcher.some((m) => __evalMatch(m, url, ctx));
  }
  return false;
}

globalThis.__evalMatch = __evalMatch;

globalThis.console = {
  log(...args) {
    globalThis.__consoleLog(args.map((part) => String(part)).join(" "));
  },
};

// Minimal URLSearchParams + URL implementations for `rewrite` rules. QuickJS has
// no built-in URL/URLSearchParams, so the host (Rust) constructs these from the
// parsed URL and reads the mutated state back. Serialization fidelity is handled
// on the Rust side from `_pairs`, so toString() here is intentionally lightweight.
class URLSearchParams {
  constructor(init) {
    this._pairs = [];
    if (Array.isArray(init)) {
      for (const pair of init) {
        this._pairs.push([String(pair[0]), String(pair[1])]);
      }
    } else if (typeof init === "string") {
      for (const [key, value] of __parseQuery(init)) {
        this._pairs.push([key, value]);
      }
    }
  }
  get(name) {
    const found = this._pairs.find((p) => p[0] === name);
    return found ? found[1] : null;
  }
  getAll(name) {
    return this._pairs.filter((p) => p[0] === name).map((p) => p[1]);
  }
  has(name) {
    return this._pairs.some((p) => p[0] === name);
  }
  set(name, value) {
    name = String(name);
    value = String(value);
    const idx = this._pairs.findIndex((p) => p[0] === name);
    if (idx === -1) {
      this._pairs.push([name, value]);
    } else {
      this._pairs[idx][1] = value;
      this._pairs = this._pairs.filter((p, i) => p[0] !== name || i === idx);
    }
  }
  append(name, value) {
    this._pairs.push([String(name), String(value)]);
  }
  delete(name) {
    this._pairs = this._pairs.filter((p) => p[0] !== name);
  }
  forEach(cb) {
    for (const pair of this._pairs) cb(pair[1], pair[0], this);
  }
  toString() {
    return this._pairs
      .map((p) => __formEncode(p[0]) + "=" + __formEncode(p[1]))
      .join("&");
  }
}

// `_pairs` always holds *decoded* values: the Rust side builds them from
// `query_pairs()` and reads them back through `append_pair`. Serializing and
// parsing must therefore encode and decode in step, or any value containing
// `&` or `=` — i.e. every wrapped URL — splits into extra parameters on the
// `url.search` -> `new URLSearchParams(...)` round-trip that `rewrite` rules use.
function __formEncode(value) {
  return encodeURIComponent(String(value)).replace(/%20/g, "+");
}

function __formDecode(value) {
  // '+' means space only before percent-decoding, so an encoded %2B survives.
  const spaced = String(value).replace(/\+/g, " ");
  try {
    return decodeURIComponent(spaced);
  } catch (err) {
    // Malformed escapes (`%zz`, a trailing `%`) must not kill the rewrite rule;
    // leave them as-is, like the WHATWG parser does.
    return spaced;
  }
}

function __parseQuery(search) {
  let s = String(search == null ? "" : search);
  if (s.startsWith("?")) s = s.slice(1);
  if (!s) return [];
  return s.split("&").map((seg) => {
    const i = seg.indexOf("=");
    return i === -1
      ? [__formDecode(seg), ""]
      : [__formDecode(seg.slice(0, i)), __formDecode(seg.slice(i + 1))];
  });
}

class __SuperSurferURL {
  constructor(parts) {
    this.protocol = parts.protocol;
    this.username = parts.username;
    this.password = parts.password;
    this.hostname = parts.hostname;
    this.port = parts.port;
    this.pathname = parts.pathname;
    this.hash = parts.hash;
    this._params = new URLSearchParams(parts.pairs);
  }
  get searchParams() {
    return this._params;
  }
  get search() {
    const s = this._params.toString();
    return s ? "?" + s : "";
  }
  set search(value) {
    this._params = new URLSearchParams(__parseQuery(value));
  }
  get host() {
    return this.port ? this.hostname + ":" + this.port : this.hostname;
  }
  get href() {
    let auth = "";
    if (this.username) {
      auth = this.username + (this.password ? ":" + this.password : "") + "@";
    }
    return (
      this.protocol + "//" + auth + this.host + this.pathname + this.search + this.hash
    );
  }
  toString() {
    return this.href;
  }
}

globalThis.__makeSearchParams = function (pairs) {
  return new URLSearchParams(pairs);
};

globalThis.__makeMutableUrl = function (parts) {
  return new __SuperSurferURL(parts);
};
