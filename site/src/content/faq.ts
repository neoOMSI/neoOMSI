export interface Question {
  q: string;
  a: string;
}

export const FAQ: Question[] = [
  {
    q: "What is neoOMSI?",
    a: "neoOMSI is a free, open-source project rebuilding the OMSI 2 simulator on a modern 64-bit engine. The goal is to support your existing maps, buses and add-ons while reproducing the original game as closely as possible.",
  },
  {
    q: "Is neoOMSI finished?",
    a: "Not yet. neoOMSI is still in early development. The available Nightly builds are intended for testing and can have bugs, missing features and performance issues. Stable releases are planned for later.",
  },
  {
    q: "Is neoOMSI free?",
    a: "Yes. neoOMSI is free to download and its source code is available on GitHub. You still need your own copy of OMSI 2 for its maps, buses and other game files.",
  },
  {
    q: "Do I need OMSI 2 to play neoOMSI?",
    a: "Yes. neoOMSI does not include OMSI 2 maps, buses or other game assets. On first launch, select your OMSI 2 folder containing the maps and Vehicles directories.",
  },
  {
    q: "Does neoOMSI work with my OMSI 2 maps, buses and mods?",
    a: "That is our goal, but compatibility is not complete yet. Existing content may work without conversion, but some maps and buses still have missing features, behave differently or fail to load. Please report content that does not work as expected.",
  },
  {
    q: "Can I play OMSI 2 on a Mac?",
    a: "Yes, with neoOMSI's experimental macOS builds for Apple silicon (M1 or newer) and Intel Macs on macOS 11 or newer. Copy the files from your OMSI 2 installation to your Mac, then select that folder in the launcher.",
  },
  {
    q: "Can I play OMSI 2 on Linux?",
    a: "Yes, through neoOMSI's experimental Linux builds for x86-64 and ARM64 systems with Vulkan support. neoOMSI runs natively, without Wine or Proton, using your existing OMSI 2 files.",
  },
  {
    q: "Can I play OMSI 2 on Android?",
    a: "No. neoOMSI does not offer or support an Android or mobile version. The available downloads are for Windows, macOS and Linux.",
  },
  {
    q: "Which platforms does neoOMSI support?",
    a: "Development builds are available for Windows 10/11 (x64), Windows 11 on ARM, macOS 11 or newer (Apple silicon and Intel), and Linux (x64 and ARM64). Dedicated server packages are available for Windows and Linux.",
  },
  {
    q: "Does neoOMSI change my OMSI 2 installation?",
    a: "No. neoOMSI only reads files from your OMSI 2 folder. Add-ons can be placed into a separate Mods folder next to neoOMSI, or managed through the launcher's Mods page, keeping your original files untouched.",
  },
  {
    q: "Does neoOMSI remove the memory limits of OMSI 2?",
    a: "Yes. neoOMSI uses a 64-bit engine, so it does not have OMSI 2's 32-bit process memory limit. That does not guarantee better frame rates; performance still depends on your hardware and ongoing optimization.",
  },
  {
    q: "Which graphics APIs does neoOMSI use?",
    a: "neoOMSI renders through the wgpu graphics library, which uses DirectX 12 on Windows, Metal on macOS, and Vulkan on Linux.",
  },
  {
    q: "Does neoOMSI support multiplayer?",
    a: "neoOMSI includes multiplayer support and dedicated server builds for Windows and Linux. Servers can run without a graphics card or game window. Multiplayer is still under development.",
  },
  {
    q: "What is the difference between neoOMSI and openOMSI?",
    a: "neoOMSI is an independent project based on an earlier, MIT-licensed version of openOMSI. We focus on reproducing OMSI 2 behavior, reviewing changes and testing compatibility. Both projects continue to be developed separately.",
  },
  {
    q: "Is neoOMSI affiliated with the makers of OMSI 2?",
    a: "No. neoOMSI is an independent community project and is not affiliated with, endorsed by, or sponsored by MR Software or Aerosoft. It does not include proprietary binaries or assets from OMSI 2.",
  },
  {
    q: "How do I install mods in neoOMSI?",
    a: "Put your add-ons in the Mods folder next to neoOMSI, or add them through the Mods page in the launcher. They are loaded separately, without modifying the original OMSI 2 files.",
  },
  {
    q: "How do I report a bug in neoOMSI?",
    a: "Report the problem through our GitHub issue tracker. Include your neoOMSI version, operating system, affected map and bus, and the steps needed to reproduce it. A comparison with OMSI 2 is helpful if you can provide one.",
  },
];

export const HOME_FAQ = FAQ.filter((f) =>
  [
    "What is neoOMSI?",
    "Do I need OMSI 2 to play neoOMSI?",
    "Is neoOMSI finished?",
    "Does neoOMSI work with my OMSI 2 maps, buses and mods?",
    "What is the difference between neoOMSI and openOMSI?",
  ].includes(f.q),
);

export const OPENOMSI_FAQ: Question[] = [
  {
    q: "Is neoOMSI the same as openOMSI?",
    a: "No. neoOMSI is an independent project with its own repository, maintainers, code review policy, and release cycle.",
  },
  {
    q: "Does neoOMSI share code with openOMSI?",
    a: "Yes. neoOMSI started from an earlier MIT-licensed version of openOMSI. Both projects have developed independently since then. The original copyright and license notices are preserved in the neoOMSI repository.",
  },
  {
    q: "Is neoOMSI an official successor to openOMSI?",
    a: "No. neoOMSI is an independent project, not the official successor to openOMSI. Neither team maintains the other project.",
  },
  {
    q: "Do both projects need original OMSI 2 content?",
    a: "Yes. Neither project distributes copyrighted OMSI 2 assets. Both read maps, vehicles, scripts and textures from an existing OMSI 2 installation provided by the player.",
  },
  {
    q: "How do I try neoOMSI alongside openOMSI?",
    a: "Extract neoOMSI into its own folder and select the same OMSI 2 installation used by openOMSI. Keep your add-ons in neoOMSI's separate Mods folder. You can try both programs without replacing your OMSI 2 installation.",
  },
  {
    q: "Where can I find openOMSI?",
    a: "openOMSI is maintained at github.com/openOMSI-Project/openOMSI. neoOMSI downloads and source code are available through neoOMSI's repository and website.",
  },
];
