// Matcher helpers and config utilities injected before user config.

function host(hostname) {
  return (url) => url.hostname === hostname;
}

function domain(name) {
  return (url) => globalThis.__domainMatch(url.hostname, name);
}

function suffix(s) {
  const bare = s.startsWith(".") ? s.slice(1) : s;
  return (url) => url.hostname === bare || url.hostname.endsWith(s);
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
  constructor(pairs) {
    this._pairs = [];
    if (Array.isArray(pairs)) {
      for (const pair of pairs) {
        this._pairs.push([String(pair[0]), String(pair[1])]);
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
    return this._pairs.map((p) => p[0] + "=" + p[1]).join("&");
  }
}

function __parseQuery(search) {
  let s = String(search == null ? "" : search);
  if (s.startsWith("?")) s = s.slice(1);
  if (!s) return [];
  return s.split("&").map((seg) => {
    const i = seg.indexOf("=");
    return i === -1 ? [seg, ""] : [seg.slice(0, i), seg.slice(i + 1)];
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
