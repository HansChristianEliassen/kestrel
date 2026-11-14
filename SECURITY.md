# Security policy

## Supported versions
Only the [latest release](https://github.com/HansChristianEliassen/kestrel/releases/latest) gets security fixes. Kestrel can update itself (Settings > Updates), so please check you're on the latest version before reporting.

## Reporting a vulnerability
Please **don't open a public issue** for security problems.

Report it privately through GitHub instead: go to this repository's **Security** tab and click **Report a vulnerability**. Only the maintainer can see the report.

Please include:
- what an attacker could do, and what they would need (for example local access, or the ability to publish a GitHub release);
- the Kestrel version and Windows 11 build;
- steps to reproduce, or a proof of concept.

You should get a reply within 7 days. Once a fix is released, you'll be credited in the release notes unless you'd rather not be.

## Particularly interesting areas
- **The updater** (`src/update/`): anything that makes Kestrel install a file that isn't signed with the release key, install an older version, or write outside its own folder.
- **Deleting files** (`src/cleaner/cleanup.rs`): anything that makes Kestrel delete outside the folders it scanned, for example through links or junctions.
- **Running programs** (`src/uninstaller/`, `src/system/`): anything that gets a command run with more rights than the user started Kestrel with.
- **Settings it changes** (`src/optimizer/`, `src/gamemode/`): anything that leaves a setting changed that Restore everything can't put back.

## Out of scope
- Problems that need someone who can already sign releases with the Kestrel release key, or who already has administrator rights on the PC.
- SmartScreen warnings: the exe isn't signed with a Windows code-signing certificate yet (see the README).
