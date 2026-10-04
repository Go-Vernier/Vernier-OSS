// The five release targets and what each one is called on npm and in a
// GitHub release. Keep in step with the build matrix in
// .github/workflows/release.yml and PACKAGES in npm/cli/lib/platform.js.

export const TARGETS = [
  { triple: "aarch64-apple-darwin", os: "darwin", cpu: "arm64", archive: "tar.gz" },
  { triple: "x86_64-apple-darwin", os: "darwin", cpu: "x64", archive: "tar.gz" },
  { triple: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", archive: "tar.gz" },
  { triple: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", archive: "tar.gz" },
  { triple: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", archive: "zip" },
];

export const npmDir = (t) => `cli-${t.os}-${t.cpu}`;
export const npmName = (t) => `@go-vernier/${npmDir(t)}`;
export const assetName = (t) => `vernier-${t.triple}.${t.archive}`;
export const binaryName = (t) => (t.os === "win32" ? "vernier.exe" : "vernier");
