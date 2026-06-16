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
