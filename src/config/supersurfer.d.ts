export type Platform = "macos" | "windows" | "linux";

export interface Opener {
  name: string;
  bundleId?: string;
  path?: string;
}

export interface Context {
  opener?: Opener;
  platform: Platform;
  modifiers: {
    shift: boolean;
    alt: boolean;
    ctrl: boolean;
    cmd: boolean;
  };
}

export type Matcher =
  | ((url: URL, ctx: Context) => boolean)
  | Matcher[];

export interface RewriteRule {
  match: Matcher;
  url: (url: URL) => void;
}

export type BrowserTarget =
  | string
  | {
      browser: string;
      profile?: string;
      private?: boolean;
    }
  | {
      name: string;
      profile?: string;
    }
  | {
      app: string;
    }
  | {
      exe: string;
    };

export interface HandlerRule {
  match: Matcher;
  browser: BrowserTarget | ((url: URL) => BrowserTarget);
}

export interface RouterConfig {
  defaultBrowser: string;
  handlers: HandlerRule[];
  rewrite?: RewriteRule[];
  urlCleaning?: "off" | "default" | unknown[];
}

export function host(hostname: string): Matcher;
export function domain(domain: string): Matcher;
export function suffix(suffix: string): Matcher;
export function glob(pattern: string): Matcher;
export function path(pattern: string): Matcher;
export function regex(pattern: RegExp): Matcher;
export function all(...matchers: Matcher[]): Matcher;
export function not(matcher: Matcher): Matcher;

declare const host: typeof import("./supersurfer").host;
declare const domain: typeof import("./supersurfer").domain;
declare const suffix: typeof import("./supersurfer").suffix;
declare const glob: typeof import("./supersurfer").glob;
declare const path: typeof import("./supersurfer").path;
declare const regex: typeof import("./supersurfer").regex;
declare const all: typeof import("./supersurfer").all;
declare const not: typeof import("./supersurfer").not;
