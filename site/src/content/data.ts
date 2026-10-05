export const SITE = "https://neoomsi.com/";
export const REPO = "neoOMSI/neoOMSI";
export const DISCORD = "https://discord.gg/Gk7EngX6JK";
export const OPENOMSI = {
  repo: "https://github.com/openOMSI-Project/openOMSI",
  site: "https://openomsi-project.github.io/openOMSI/",
};

export const DOCS = [
  {
    file: "USER_GUIDE",
    slug: "user-guide",
    title: "User guide",
    group: "Playing",
    icon: "directions_bus",
  },
  {
    file: "COMPATIBILITY",
    slug: "compatibility",
    title: "Compatibility",
    group: "Playing",
    icon: "sync_alt",
  },
  {
    file: "SERVER",
    slug: "server",
    title: "Dedicated server",
    group: "Playing",
    icon: "dns",
  },
  {
    file: "BUILDING",
    slug: "building",
    title: "Building",
    group: "Contributing",
    icon: "construction",
  },
  {
    file: "DEVELOPMENT",
    slug: "development",
    title: "Development workflow",
    group: "Contributing",
    icon: "alt_route",
  },
  {
    file: "ISSUE_TRIAGE",
    slug: "issue-triage",
    title: "Issue triage",
    group: "Contributing",
    icon: "flag",
  },
  {
    file: "RELEASING",
    slug: "releasing",
    title: "Releasing & versioning",
    group: "Contributing",
    icon: "inventory_2",
  },
];

export interface Build {
  key: string;
  name: string;
  family: string;
  arch: string;
  note: string;
  os?: string;
  ext?: string;
}

export const PLATFORMS: Build[] = [
  {
    key: "windows-x64",
    name: "Windows",
    family: "Windows",
    arch: "x64",
    note: "Almost every PC, Windows 10 or newer",
    os: "windows",
  },
  {
    key: "windows-arm64",
    name: "Windows on ARM",
    family: "Windows",
    arch: "ARM64",
    note: "Snapdragon laptops, Windows 11",
    os: "windows-arm",
  },
  {
    key: "macos-arm64",
    name: "macOS (Apple silicon)",
    family: "macOS",
    arch: "Apple silicon",
    note: "M1 or newer, macOS 11 or newer",
    os: "mac",
  },
  {
    key: "macos-x64",
    name: "macOS (Intel)",
    family: "macOS",
    arch: "Intel",
    note: "Intel Macs, macOS 11 or newer",
    os: "mac-intel",
  },
  {
    key: "linux-x64",
    name: "Linux",
    family: "Linux",
    arch: "x64",
    note: "x86-64 with Vulkan drivers",
    os: "linux",
  },
  {
    key: "linux-arm64",
    name: "Linux on ARM",
    family: "Linux",
    arch: "ARM64",
    note: "ARM64 with Vulkan drivers",
    os: "linux-arm",
  },
  {
    key: "android-arm64",
    ext: "apk",
    name: "Android",
    family: "Android",
    arch: "APK",
    note: "arm64; built locally, not in automated releases",
    os: "android",
  },
];

export const SERVERS: Build[] = [
  {
    key: "server-windows-x64",
    name: "Server for Windows",
    family: "Windows",
    arch: "x64",
    note: "No window, no GPU needed",
  },
  {
    key: "server-windows-arm64",
    name: "Server for Windows on ARM",
    family: "Windows",
    arch: "ARM64",
    note: "No window, no GPU needed",
  },
  {
    key: "server-linux-x64",
    name: "Server for Linux",
    family: "Linux",
    arch: "x64",
    note: "No window, no GPU needed",
  },
  {
    key: "server-linux-arm64",
    name: "Server for Linux on ARM",
    family: "Linux",
    arch: "ARM64",
    note: "Raspberry Pi 4/5 and ARM VPS",
  },
];

function visitorOs() {
  const ua = navigator.userAgent || "";
  if (/Android/i.test(ua)) return "android";
  if (/Windows/i.test(ua))
    return /ARM|aarch64/i.test(ua) ? "windows-arm" : "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "mac";
  if (/Linux/i.test(ua))
    return /aarch64|arm64/i.test(ua) ? "linux-arm" : "linux";
  return "";
}

export const visitorBuild = () => PLATFORMS.find((p) => p.os === visitorOs());
