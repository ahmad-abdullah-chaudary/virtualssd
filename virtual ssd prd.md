# VirtualSSD — Production-Grade Windows Cloud-Backed Virtual Block Storage

## ROLE AND MISSION

Act as a multidisciplinary senior engineering team consisting of:

- A Windows storage and device-driver engineer
- A distributed-storage and filesystem engineer
- A Rust systems engineer
- A Windows desktop application engineer
- A Google Drive API integration engineer
- A Telegram TDLib integration engineer
- A security engineer
- A QA, reliability and release engineer

Your mission is to DESIGN, IMPLEMENT, TEST, DOCUMENT AND PACKAGE **VirtualSSD**, a Windows desktop application that creates persistent virtual block disks backed by the user's own Google Drive and Telegram accounts.

This is a real storage-engineering project, not a mockup, demo dashboard, simulated cloud drive, or ordinary filesystem mount.

Do not stop after generating a plan or architecture document. Work in the actual repository, create the implementation files, compile the code, run the tests, fix errors, and maintain a clear record of what is implemented versus what remains incomplete.

Do not claim that any feature works unless it has been implemented and verified with appropriate tests.

## 1. PRODUCT IDENTITY

Product name: VirtualSSD

Product category: Windows cloud-backed virtual block storage.

Tagline: Your cloud. Your virtual disk. Your data, protected.

Primary operating system: Windows 11 x64, with Windows 10 support considered only after compatibility testing.

Initial target: A single Windows computer, multiple Windows user accounts, and independently mounted virtual disks.

Core providers:

1. Google Drive through the official Google Drive API and OAuth 2.0.
2. Telegram through the official Telegram client API, preferably using TDLib.

Do not implement TeraBox, Google Cloud Storage, unofficial scraping, browser-cookie extraction, or reverse-engineered private endpoints in this release.

Do not confuse Google Drive with Google Cloud Storage. This project uses Google Drive.

## 2. PRODUCT REQUIREMENTS

### 2.1 Actual virtual disks

VirtualSSD must ultimately create actual Windows-visible block devices, not merely drive letters backed by a filesystem.

Windows should be able to discover the device and, when supported by the selected architecture, initialize it, create partitions, format volumes, assign drive letters, and access the resulting filesystem.

A virtual disk must expose a well-defined capacity and logical sector geometry. Its read, write, flush, device-removal, and error-reporting behavior must comply with the Windows storage interfaces selected for implementation.

Investigate the current Windows Driver Kit, documented virtual storage architectures, and supported storage-management interfaces before choosing the implementation.

Explicitly distinguish between:

- A virtual block device
- A virtual hard-disk container such as VHD/VHDX
- A user-mode filesystem
- A cloud-backed filesystem
- A mounted cloud folder

WinFsp alone is not a block-device implementation. Do not present a filesystem mount as a completed block-device feature.

Choose a technically defensible architecture and document why it was selected. If a kernel-mode driver is necessary, use documented Windows driver interfaces and a signed-driver release strategy. Keep kernel code minimal and isolate risky operations from the desktop UI.

Do not attempt to bypass Windows driver-signing or security protections.

### 2.2 Separate disks for separate providers

Users must be able to create independent disks, such as:

- VirtualSSD — Google Drive
- VirtualSSD — Telegram

Each disk must have its own stable disk ID, provider binding, capacity, logical sector size, chunk mapping, metadata, journal, cache namespace, health state, and recovery information.

The disk's identity must not depend on its displayed drive letter.

Prevent duplicate mounts and prevent two conflicting writable mounts of the same disk unless a fully tested coordination protocol is later implemented.

### 2.3 Desktop control application

Build a polished, production-oriented Windows desktop application.

Preferred stack:

- Tauri for the desktop shell
- React, TypeScript, and Tailwind CSS for the interface
- Rust for the storage engine, provider adapters, metadata, cache, and orchestration
- SQLite with appropriate transactions and write-ahead logging for local metadata where suitable
- Windows Credential Manager and/or DPAPI for protecting local credentials and secrets
- Windows Driver Kit and a documented Windows device architecture where required

Use pnpm for the JavaScript workspace and Cargo for Rust.

Avoid unnecessary services, paid dependencies, and avoidable infrastructure costs. Do not add external AI APIs or subscription-based dependencies.

The interface should contain:

1. Dashboard with disk status and storage statistics.
2. Provider accounts page.
3. Create-disk wizard.
4. Disk details and management.
5. Cache configuration and usage.
6. Active transfer queue.
7. Storage health and integrity status.
8. Error logs and diagnostics.
9. Recovery and repair workflow.
10. Application settings, updates, and security information.

Display truthful states such as Connecting, Online, Degraded, Offline, Recovering, Read-only, Unmounted, and Needs Attention where applicable.

Never use hardcoded usage statistics or fake provider connection indicators in the production application.

### 2.4 Provider authentication

Google Drive:

- Use the official Google Drive API.
- Implement OAuth 2.0 using a flow appropriate for a native desktop application.
- Request the minimum necessary scopes.
- Support account connection, disconnection, token refresh, authorization expiry, quota errors, retries, cancellation, and reconnection.
- Store remote objects in an application-managed folder with stable identifiers.
- Use supported upload, download, revision and metadata mechanisms.
- Prefer resumable uploads for large objects.
- Handle ambiguous upload outcomes without blindly duplicating or overwriting committed objects.
- Document Google OAuth consent-screen and production verification requirements.

Telegram:

- Use TDLib or another documented official Telegram client implementation.
- Support legitimate user authentication, session persistence, reauthentication, account disconnection, upload and download operations.
- Keep API credentials and user sessions out of source control, logs, screenshots and crash reports.
- Maintain durable mappings between storage objects and their Telegram remote references.
- Test current file-size restrictions, transfer behavior, rate limits, account restrictions and recovery scenarios.
- Do not treat Telegram messages or media as guaranteed archival storage.
- Do not promise unlimited capacity, permanent retention, unlimited request rates or uninterrupted access.

Both adapters must implement a shared internal provider contract without pretending that their capabilities and failure modes are identical.

## 3. STORAGE ENGINE ARCHITECTURE

Implement clearly separated modules for:

- Virtual disk lifecycle and device management
- Block I/O scheduling
- Logical block addressing
- Chunk mapping
- Remote object storage
- Local persistent cache
- Write-ahead journal
- Manifest and metadata management
- Integrity verification
- Transfer scheduling
- Retry and backoff policies
- Recovery and repair
- Provider abstraction
- Authentication and secrets
- IPC and access control
- Diagnostics and telemetry

Keep the core storage engine independent of the user interface.

Define stable, versioned internal interfaces and documented state machines. Use structured errors rather than generic success/failure booleans.

### 3.1 Chunk storage

Use a configurable chunk size. Start with a candidate of 4 MiB for benchmarks, not an assumption that this is universally optimal.

Every chunk must have a stable logical identity and sufficient metadata to locate and verify its remote representation.

Store, as appropriate:

- Disk ID
- Logical chunk index
- Chunk length
- Format version
- Cryptographic checksum
- Remote object identifier
- Object version or revision information
- Local cache state
- Commit state
- Encryption metadata when encryption is enabled

Support sparse, unwritten regions without unnecessarily uploading zero-filled data.

Do not assume that a remote upload succeeded just because a request was sent. Confirm the result using the provider's supported metadata or retrieval mechanisms.

Use immutable or versioned chunk objects where practical. Do not overwrite the only known-good representation before a replacement is recoverable.

### 3.2 Manifest and metadata

Design a versioned disk manifest that maps logical regions to chunk representations.

The manifest must support:

- Stable disk identity
- Capacity and sector geometry
- Schema versioning
- Provider identity
- Chunk mappings
- Object versions
- Integrity metadata
- Commit identifiers
- Recovery metadata
- Format migration

Protect manifest updates from partial publication.

Publish a new authoritative manifest only after its referenced data has reached the durability level required by the disk's configured policy.

Retain enough prior metadata to recover from interrupted commits. Define safe cleanup rules for obsolete chunks so garbage collection cannot delete data still referenced by a recoverable manifest.

Use explicit migration procedures for schema changes. Never silently replace an unreadable manifest with a new empty disk.

### 3.3 Journal and crash consistency

Implement a documented transaction protocol.

At minimum, design and test these stages:

1. Receive and validate a write.
2. Record the operation and required recovery information.
3. Persist required local journal data.
4. Generate new chunk versions.
5. Upload and verify required remote objects.
6. Publish the corresponding manifest update.
7. Persist the committed state.
8. Acknowledge the operation according to the defined durability contract.
9. Reclaim obsolete objects only when safe.

The exact sequence may differ according to the chosen storage architecture, but it must be formally documented and validated through crash-injection tests.

Recovery must be idempotent: repeating a recovery step must not corrupt data, produce conflicting authoritative manifests, or discard the last valid committed state.

Handle crashes during every transaction stage.

### 3.4 Write durability contract

Define precisely what a successful write, flush, and filesystem-visible operation guarantee.

Support a conservative default policy in which successful durability acknowledgements are issued only after the necessary data and metadata have been committed according to the documented policy.

If the network is unavailable, do not report remotely committed data when it is only present in volatile memory or an unprotected cache.

An explicitly configured write-back mode may be considered later, but only if:

- Pending writes are recorded in a durable local journal.
- The UI reports pending remote synchronization.
- The behavior of flush and power loss is documented.
- Cache loss and disk-full scenarios are handled.
- The storage engine never silently drops acknowledged writes.
- The mode's guarantees are tested and accurately described.

Do not weaken the default durability policy merely to improve benchmark results.

### 3.5 Cache behavior

Implement a persistent, bounded cache with configurable storage limits.

Track clean, dirty, pending, committed, evicted, missing and invalid data states as appropriate.

Requirements:

- Cache hits should not require remote network access.
- Uncached online reads should fetch and verify the relevant data.
- Uncached offline reads must return a truthful I/O error rather than fabricated bytes.
- Dirty data must never be evicted unless it is safely persisted according to the active policy.
- Cache corruption must be detected.
- Disk-full conditions must be handled without silently discarding data.
- Cache metadata must survive normal restarts.
- Cache cleanup must be safe during interrupted operations.

Expose cache usage, hit rate, misses, pending uploads, and errors.

### 3.6 Integrity

Use a suitable cryptographic hash, such as SHA-256 or BLAKE3, for chunk integrity. Choose one and document the trade-offs.

Validate data after downloads and before accepting remote objects as committed.

Differentiate between corruption, unavailable data, authorization errors, network failures, quota errors and manifest inconsistency.

Provide a verification command and an integrity report.

Do not claim that a checksum alone provides confidentiality or protection against every malicious modification.

### 3.7 Optional encryption

Design the architecture to support client-side encryption at rest in the cloud.

Do not make encryption mandatory for the first prototype if it would block validation of the core storage engine.

Before shipping encryption, implement and test:

- Authenticated encryption using a well-maintained cryptographic library.
- Secure key generation and separation from stored data.
- Secure key storage and recovery procedures.
- Key rotation and versioning.
- Nonce uniqueness.
- Authentication-tag verification.
- Behavior when a key is lost or invalid.
- Avoidance of secret leakage through logs, metadata and crash reports.

Do not invent a custom cryptographic algorithm.

Explain which metadata remains visible to each cloud provider.

## 4. MULTI-USER SECURITY

Design for multiple Windows users on the same computer.

Each user's accounts, tokens, sessions, cache and disk configuration must be isolated.

Use Windows access controls and an appropriately secured IPC mechanism, such as named pipes with explicit ACLs, between the desktop application and any privileged storage service.

The service must validate caller identity and authorize every disk-management operation.

Do not expose an unauthenticated local control port.

Do not trust UI-supplied disk IDs, paths or provider references without validation.

Prevent:

- Cross-user disk access
- Unauthorized mounting or unmounting
- Token leakage
- Path traversal
- Arbitrary file deletion
- Unsafe deserialization
- Command injection
- Logging credentials or authentication codes
- Unsafe automatic repair
- Unauthorized access to raw disk contents

Create a threat model and security test suite.

## 5. OFFLINE AND FAILURE BEHAVIOR

Treat the cloud connection as unreliable.

Handle:

- Internet loss during a read
- Internet loss during a write
- Interrupted uploads and downloads
- Token expiration
- Account disconnection
- API throttling
- Provider outages
- Quota exhaustion
- Local disk exhaustion
- Process termination
- Windows restart
- Sudden power loss
- Corrupt cache
- Missing remote objects
- Stale manifests
- Conflicting metadata
- Failed device removal
- Driver or service failure

Use bounded exponential backoff with jitter where appropriate.

Make retries safe for operations whose result is uncertain.

Never spin indefinitely, silently convert errors to success, or repeatedly retry an operation that can cause destructive side effects.

On shutdown, attempt a controlled flush and unmount. If safe shutdown cannot be completed, clearly communicate the problem and preserve recovery information.

Do not promise seamless offline access to data that has never been cached.

## 6. DISK LIFECYCLE

Implement these lifecycle operations:

- Create a disk
- Validate capacity and provider configuration
- Allocate a stable disk ID
- Initialize persistent metadata
- Mount
- Unmount
- Eject or remove safely
- Inspect
- Verify integrity
- Recover
- Repair where safe
- Export diagnostic information
- Delete a disk with explicit confirmation

Deletion must distinguish between removing a local configuration and permanently deleting remote data.

Never delete cloud objects automatically merely because a local disk configuration is missing.

For the first release, require explicit confirmation for destructive operations. Prefer dry-run previews for remote deletion and repair.

Disk creation must not overwrite an existing disk or manifest.

Do not support booting Windows from VirtualSSD in the initial release.

## 7. USER EXPERIENCE

Create a professional Windows application with a clean, modern interface and accessible controls.

The interface must communicate storage state, not hide it.

Display, where available:

- Virtual disk capacity
- Logical disk status
- Provider connection state
- Local cache consumption
- Committed remote data
- Pending synchronization
- Transfer speeds
- Queue length
- Last successful verification
- Last error and recovery recommendation

Use clear warnings before destructive operations.

Avoid fake SSD performance claims. The interface must not imply that cloud-backed storage has the latency or durability characteristics of a physical NVMe SSD.

Use background tasks and cancellation safely. Keep UI responsiveness independent of large transfers.

## 8. PROJECT ORGANIZATION

Create a maintainable repository. Adapt this structure when justified by the selected device architecture.

virtualssd/
  apps/
    desktop/
      src/
        pages/
        components/
        stores/
        services/
      src-tauri/
  services/
    storage-service/
      src/
        disk_manager/
        block_engine/
        journal/
        cache/
        metadata/
        recovery/
        ipc/
      tests/
  drivers/
    windows-virtual-disk/
      src/
      tests/
      docs/
  crates/
    storage-core/
    block-map/
    manifest/
    journal/
    cache-manager/
    integrity/
    provider-contract/
    provider-gdrive/
    provider-telegram/
    security/
  schemas/
  tests/
    unit/
    integration/
    crash-recovery/
    security/
    provider-conformance/
    performance/
  installer/
  scripts/
  docs/
    architecture.md
    threat-model.md
    durability-contract.md
    recovery.md
    provider-compatibility.md
    driver-installation.md
    troubleshooting.md
    release-process.md
  Cargo.toml
  pnpm-workspace.yaml
  README.md
  LICENSE
  SECURITY.md

Use stable interfaces between modules. Avoid circular dependencies and giant monolithic source files.

Add an architecture decision record for every major decision, especially the Windows block-device implementation, commit protocol, provider mapping and durability policy.

## 9. DEVELOPMENT PHASES

Execute the project in gated phases. Do not bypass a gate because the UI looks finished.

### Phase 0 — Research and feasibility

Inspect the development environment, Windows version, installed Rust toolchain, pnpm, Node.js, Visual Studio Build Tools and Windows Driver Kit availability.

Review current official documentation for the Windows device architecture, Google Drive API, OAuth 2.0, TDLib and the selected installer/signing approach.

Determine the most defensible implementation path for a real Windows-visible block device.

Document dependencies, limitations, required developer accounts, build tools, signing constraints and costs.

Produce an architecture decision record before implementing the virtual disk.

Do not assume that a user-mode filesystem or VHD/VHDX container automatically provides cloud-backed block I/O.

### Phase 1 — Local-only block device

Build the smallest real prototype that exposes a virtual block disk to Windows.

Initially use a disposable local backing store so the block engine can be tested without cloud failure modes.

Prove that Windows can discover the device and that the supported initialization, partitioning, formatting and read/write workflows operate correctly.

Test sector geometry, alignment, flush, device removal and errors.

If a real block device cannot be built in the current environment, state the exact blocker. Do not claim that a placeholder is a functioning driver.

### Phase 2 — Durable storage core

Implement chunk mapping, metadata, journaling, checksums, persistent caching and recovery.

Write unit tests for boundary conditions, overlapping writes, partial chunks, sparse regions and interrupted transactions.

Add deterministic crash injection at every commit stage.

Pass recovery and data-integrity tests before moving forward.

### Phase 3 — Google Drive adapter

Implement OAuth, account management, remote chunk storage, resumable transfers, retries, metadata verification and manifest publication.

Use a dedicated test account and disposable test data.

Validate read-after-write behavior, quota errors, authorization expiry, ambiguous upload outcomes, deletion protection and recovery.

Do not use production user data during development.

### Phase 4 — Telegram adapter

Integrate TDLib and legitimate Telegram user authentication.

Implement persistent remote references, transfer management, verification, retries and provider-specific error handling.

Test current API constraints and failure cases.

Do not assume Telegram offers the same object lifecycle or guarantees as Google Drive.

### Phase 5 — Desktop control hub

Implement real account management, disk creation and lifecycle controls, status monitoring, transfer queue, cache settings, diagnostics and recovery workflows.

Connect every control to a working backend operation.

Remove mock statistics, placeholder success messages and fake connections before release.

### Phase 6 — Multi-user security and hardening

Implement IPC authorization, user isolation, credential protection, safe shutdown, recovery tooling and installer behavior.

Run threat-model-based tests and destructive-failure simulations.

### Phase 7 — Release candidate

Build reproducibly on a documented Windows environment.

Run the full test suite, produce release artifacts, validate the installation and uninstallation paths, document signing requirements and generate a release report.

Do not label the product production-ready if any mandatory acceptance test remains unverified.

## 10. MANDATORY ACCEPTANCE TESTS

Create automated tests wherever possible and record actual outcomes.

### Block-device tests

- Device discovery by Windows
- Correct capacity and logical sector geometry
- Supported disk initialization
- Partition creation and formatting
- Read/write round-trip equality
- Aligned and unaligned I/O
- Boundary and overlapping writes
- Flush semantics
- Safe mount and unmount
- NTFS file creation, modification, rename and deletion

### Persistence tests

- Close and reopen a disk without data loss
- Restart the storage service
- Reboot Windows and recover a previously committed disk
- Crash during chunk upload
- Crash during manifest publication
- Crash during journal cleanup
- Recover the last valid committed state
- Detect corrupt chunks and manifests
- Never silently initialize a missing or unreadable disk

### Cache and offline tests

- Cached reads while offline
- Uncached reads while offline return a truthful error
- No dirty-data eviction
- Cache full and disk-full handling
- Correct cache-state recovery after restart
- No false remote-commit acknowledgements
- Correct resumption after reconnection

### Provider tests

- OAuth and Telegram authentication
- Token/session expiry
- Reauthentication
- Interrupted transfers
- Rate limits and retries
- Quota exhaustion
- Missing remote objects
- Conflicting metadata
- Ambiguous upload outcomes
- Provider disconnection and reconnection

### Security tests

- Multi-user isolation
- Unauthorized IPC requests
- Malformed input
- Invalid metadata
- Secret redaction
- Credential storage and removal
- Safe handling of paths and remote identifiers
- Protection against unsafe deletion

### Release tests

- Clean installation
- Upgrade and migration
- Uninstallation behavior
- Service startup and shutdown
- Driver installation and removal
- Diagnostic export
- Recovery instructions
- Build reproducibility
- Dependency and vulnerability review

## 11. PERFORMANCE ENGINEERING

Establish reproducible benchmarks rather than inventing targets or results.

Measure:

- Random and sequential read throughput
- Random and sequential write throughput
- Small-block latency
- Large-block throughput
- Cache hit rate
- Cold-read latency
- Upload and download throughput
- CPU and memory consumption
- Metadata growth
- Recovery time
- Behavior under throttling and network interruption

Benchmark local storage, cached operations and remote operations separately.

Tune chunk size, concurrency, queue limits and cache size using real measurements.

Never compromise durability to produce impressive benchmark numbers.

## 12. ENGINEERING RULES

1. Inspect existing files before modifying them.
2. Maintain a working repository and make small, reviewable changes.
3. Use typed interfaces and explicit state machines.
4. Compile and test after each meaningful implementation step.
5. Use pinned or appropriately constrained dependencies and review security advisories.
6. Never hardcode API secrets, tokens, test passwords or personal account data.
7. Never commit generated credentials or local session files.
8. Never suppress an error just to make a test pass.
9. Never fabricate a successful test, build, upload, disk mount or recovery.
10. Never delete user data as an automatic troubleshooting action.
11. Never replace real storage behavior with mock implementations in a production code path.
12. Clearly label stubs, experimental features and unsupported capabilities.
13. Prefer small, testable modules over unnecessary abstractions.
14. Avoid introducing dependencies without a clear benefit.
15. Keep documentation aligned with the actual implementation.
16. Do not claim a feature is complete because its source code exists.
17. If an API, Windows interface or driver behavior is uncertain, verify it against authoritative documentation or a reproducible test.
18. If a blocker requires user intervention, explain the exact action required and continue with independent tasks.
19. Preserve all known-good recovery data during development.
20. Never run destructive disk operations against a user's real disks without explicit, informed confirmation.

## 13. REQUIRED DELIVERABLES

Produce:

- Complete source repository
- Architecture documentation and decision records
- Real Windows block-device prototype
- Storage engine and local recovery implementation
- Google Drive adapter
- Telegram adapter
- Desktop application
- Installer and uninstaller
- Automated test suite
- Crash-recovery test harness
- Security and threat-model documentation
- Provider setup instructions
- Google OAuth configuration instructions
- Telegram TDLib setup instructions
- Build and development instructions
- Troubleshooting and recovery manual
- Release checklist
- Performance benchmark report
- Known limitations and compatibility matrix

Provide exact commands to install dependencies, build, test, run and package the application.

List which features are complete, partially implemented, blocked or unverified.

## 14. DEFINITION OF DONE

VirtualSSD is not complete merely because the application launches or the dashboard looks polished.

The first meaningful milestone is a real, disposable, Windows-visible virtual block device that survives restart and passes read/write and recovery tests.

The first cloud-backed milestone is a disk backed by Google Drive that passes the defined persistence, integrity and failure tests.

The Telegram milestone must pass the same core provider-conformance tests where its API capabilities permit, with documented exceptions.

The production release must not contain mock storage paths, fake success states, silent data-loss behavior or undocumented durability compromises.

If a release requirement cannot be met, report the blocker honestly, preserve the working implementation, and propose the smallest safe next step.

## 15. YOUR FIRST ACTION

Start with Phase 0.

Inspect the actual development environment and repository. Research the documented Windows block-device approach. Produce the architecture decision record, the initial threat model, the proposed commit protocol, the provider capability matrix and a prioritized implementation checklist.

Then begin Phase 1 in the repository.

Do not stop at a proposal. Implement the first feasible vertical slice, build it, test it, and report the actual result before proceeding to the next phase.

**Priority order: data integrity first, recoverability second, security third, correct Windows block-device behavior fourth, provider reliability fifth, performance sixth, and UI polish after the storage foundation is sound.**