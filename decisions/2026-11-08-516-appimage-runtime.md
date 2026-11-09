# Verify the AppImage runtime before packaging

Master d9bc652 (run 42465222132) could not publish the workspace/composer update: appimagetool downloaded the moving type2 runtime during packaging and received HTTP 504. Replaying the actual cached packager against an isolated copy exposed the stderr Tauri suppressed.

Resolve the official x86_64 runtime as a separate, bounded download with retries, pin its release asset ID and SHA-256, and atomically cache only verified bytes. Verify the cache on every use. Pass that path through the output plugin’s supported LDAI_RUNTIME_FILE setting. Do not change installer targets, signing, tests, publication gates, or application behavior. An unavailable or mismatched dependency still fails the build.

The pinned asset is AppImage/type2-runtime asset 657374641, commit fedfad51acc2860b2916a1eef32ba4a500ff31ef, SHA-256 617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46. It is fetched through the release-assets API rather than the moving continuous URL.

The first PR CI run 42466631953 verified both Linux bundles, then exposed the same HTTP 504 for Tauri’s NSIS helper DLL. Tauri 2.10.4 also checks the actual NSIS.exe hash on Linux; without that file it deletes the DLL cache on every build. Prepare the official NSIS 3.13 executable and v0.5.3 helper DLL through pinned release asset IDs and SHA-256 verification. The executable’s SHA-1 matches the bundler’s required 6d45d4e8ccf18fe1fdac9164058f356102a2ecf2; its SHA-256 is 78e17de55fa013be7620487e85b6676c204c6c51d7ebabb74ee8a9cfce6252a0. This uses the real tool, no placeholder executable or skipped integrity check. Linux still uses its existing makensis to build the Windows installer.

Fresh verified NSIS resolution and cached resolution with downloads disabled pass. All 21 release regression tests pass, including rejection of invalid NSIS bytes without replacing a cached tool or leaving staging files. The next full CI must establish actual Windows packaging and all application suites; interactive Windows acceptance still needs Zero online.

Tracing the two-version update test found a separate real packaging entrypoint: desktop/tests/update-builds.sh starts in a later workflow step and does not inherit Installers’ exported environment. It now resolves the same verified runtime itself before building either AppImage, including standalone invocation. Run 42467121204 passed both platform installers and the application suite, then spent its remaining time in these update builds; its successor must test the complete corrected path.
