//! Every kind of event the log holds. The body of each row is one `Ev`.
//! Secrets never appear here: a secret write records only its name.

mod memory;
mod meta;
mod mission;
mod security;
pub use {memory::*, meta::*, mission::*, security::*};

use crate::tools::Label;
use magnus_adapter_proto::{Block, ErrorClass, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ev {
    KernelBoot {
        version: String,
        pid: u32,
        #[serde(default)]
        instructions: String,
    },
    SecretSet {
        name: String,
    },
    PolicySet {
        layer: String,
        cedar: String,
    },
    /// An admin turned a run module on or off for the organisation (None: back to the config's), for runs that start from then.
    ModuleSet {
        name: String,
        on: Option<bool>,
    },
    /// An admin enabled or disabled a model provider (an adapter).
    ProviderSet {
        id: String,
        enabled: bool,
    },
    /// The release channel the desktop apps of everyone but the owner and testers follow.
    DesktopChannelSet {
        channel: String,
    },
    /// The retired console's session lifetimes: read, and changing nothing.
    ConsoleSettings {
        idle_minutes: i64,
        session_hours: i64,
        /// The subnets it takes connections from, as an admin set them under Operations › Network (None: the config's).
        #[serde(default)]
        allow: Option<Vec<String>>,
    },
    /// An admin's draft of a policy layer (None: dropped). Nothing in force changes until it is applied.
    PolicyDrafted {
        layer: String,
        cedar: Option<String>,
    },
    KillSwitch {
        engaged: bool,
    },
    Computer(crate::computer::Change),
    Halt(crate::halts::Change),
    Payment(crate::payments::Change),
    Rule(crate::rules::Change),
    SubAgent(crate::sub_agents::SubAgent),
    Compartment(crate::compartment::Change),
    Envelope(crate::envelope::Change),
    Staged(crate::staging::Change),
    LocalComputer(crate::local::Computer),
    LocalFile(crate::local_files::Change),
    Team(crate::team::Change),
    AccountLink(crate::account_links::Change),
    Legal(crate::legal::Change),
    Push(crate::push::Change),
    Tools(crate::tool_gate::Change),
    Evidence(crate::evidence::Change), // read-back or result sources (spec rev 147)
    Spend(crate::spend::Change),
    Catalog(crate::catalog_withdrawal::Change),
    Desktop(crate::desktop::Change),
    AdminSetting(crate::admin_app::settings::Change),
    /// A fresh screenshot for a run after a person handed its computer back.
    Handback {
        run: String,
        step: u32,
        #[serde(default)]
        image_b64: String, // content; gone once its task is deleted
    },
    /// Something done in the admin console (audit: who, what, to what, from where, before/after).
    AdminAction {
        admin: String,
        #[serde(default)]
        name: String,
        action: String,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        before: Option<Value>,
        #[serde(default)]
        after: Option<Value>,
    },
    /// Lost-reset module rewound context from `from` to `to` with a note (replies remain in log).
    ContextReset {
        run: String,
        from: u32,
        to: u32,
        signal: String,
        note: String,
    },
    /// One verifier call reviewing contract criteria (`criteria`) or work against them (`work`).
    VerifierReply {
        run: String,
        purpose: String,
        model: String,
        #[serde(default)]
        blocks: Vec<magnus_adapter_proto::Block>,
        usage: magnus_adapter_proto::Usage,
    },
    /// One model call of the reviewer module: judging a send before it
    /// leaves (`send`), or the completion criteria no code check covers
    /// (`completion`).
    ReviewerReply {
        run: String,
        purpose: String,
        model: String,
        #[serde(default)]
        blocks: Vec<magnus_adapter_proto::Block>,
        usage: magnus_adapter_proto::Usage,
    },
    /// A draft written by a scoped step: the model call saw only these
    /// threads and the person's own words, and the draft's body is what it
    /// wrote. `participants` are the threads' addresses, entitled to what
    /// the draft holds: a send of it to them passes under content
    /// provenance (#178).
    DraftScoped {
        run: String,
        person: String,
        draft: String,
        threads: Vec<String>,
        #[serde(default)]
        participants: Vec<String>,
        usage: magnus_adapter_proto::Usage,
    },
    /// The proactive loop woke a review run for the person: why (cadence,
    /// overnight, a task's end (`task {id} done|failed`), an approval
    /// waiting, a schedule coming up, a commitment due), for the loop's
    /// own scheduling (#237).
    ReviewWoken {
        run: String,
        task: String,
        person: String,
        reason: String,
    },
    /// A backup of the log and the vault was written (K10).
    BackupMade {
        file: String,
        through_seq: i64,
        events: usize,
        bytes: usize,
    },
    /// The backup was restored in memory: whether it would restore.
    BackupChecked {
        file: String,
        ok: bool,
        detail: String,
    },
    /// The backups directory was mirrored off host after a backup (#297):
    /// whether the copy held, and why not.
    BackupCopied {
        file: String,
        ok: bool,
        detail: String,
    },
    /// Removed content is still in backups kept: rewriting `files` failed
    /// (why, in `detail`). Those have had the removals up to `from` only;
    /// every removal after it, through `through`, is not yet out of them.
    /// Tried again at start and hourly until `BackupsScrubbed` (#195).
    BackupScrubPending {
        from: i64,
        through: i64,
        files: Vec<String>,
        detail: String,
    },
    /// Every backup kept has had the removals through `through`.
    BackupsScrubbed {
        through: i64,
    },
    /// An appliance update began (or a rollback, to the release before):
    /// running tasks are suspended ("update") until a version is running again.
    UpdateStarted {
        from: String,
        to: String,
        rollback: bool,
    },
    /// The new release started and took over the log.
    UpdateFinished {
        from: String,
        to: String,
    },
    /// The appliance went back from `from` to `to`, which it runs now;
    /// `from` is not installed again by itself.
    UpdateRolledBack {
        from: String,
        to: String,
        reason: String,
    },
    /// A release was found and not installed: the appliance stays as it is.
    UpdateFailed {
        to: String,
        reason: String,
    },
    /// The owner added one of the person's apps (an MCP server).
    ConnectorAdded {
        connector: crate::config::Connector,
    },
    ConnectorRemoved {
        name: String,
    },
    /// The owner set (or cleared) the organisation's registration with a
    /// provider; its client ID and secret are in the vault, not here.
    RegistrationSet {
        provider: String,
        set: bool,
    },
    /// An admin turned one of the organisation's apps on or off, or set who
    /// may use it (admin console spec, Security › Connectors and
    /// credentials). `access_groups` and `blocked_groups` None keep the
    /// app's own.
    ConnectorSettings {
        name: String,
        enabled: bool,
        #[serde(default)]
        access_groups: Option<Vec<String>>,
        #[serde(default)]
        blocked_groups: Option<Vec<String>>,
    },
    /// The organisation's credential for an app (its key) was set, or
    /// revoked. The value is in the vault, never here.
    CredentialSet {
        connector: String,
        set: bool,
    },
    /// One of an app's tools classed, at its connect or by the owner (None: not offered).
    ToolClassified {
        connector: String,
        tool: String,
        risk: Option<String>,
    },
    /// The escalation module moved a lost run to a stronger model.
    Escalated {
        run: String,
        adapter: String,
        model: String,
        signal: String,
    },
    /// The kill switch sealed the vault (B11): OAuth sign-ins taken out to be
    /// revoked at their providers, and the secrets kept sealed (API keys and
    /// app secrets, which only their provider's console can revoke). Names only.
    VaultSealed {
        revoking: Vec<String>,
        kept: Vec<String>,
    },
    /// What the provider said when asked to revoke one sign-in.
    SecretRevoked {
        name: String,
        adapter: String,
        at_provider: bool,
        detail: String,
    },
    /// The identity provider's account a person is: bound at their first
    /// sign-in through it (`subject` its `sub`), or cleared by the owner
    /// (`None`) so the next sign-in binds again.
    SubjectBound {
        person: String,
        subject: Option<String>,
    },
    /// The groups and roles the identity provider gave a person at sign-in
    /// (where the owner named those claims); they replace the config's.
    RightsFromProvider {
        person: String,
        #[serde(default)]
        groups: Option<Vec<String>>,
        #[serde(default)]
        roles: Option<Vec<String>>,
    },
    /// The owner set whether people's data is private from the admin
    /// (owner's ruling 14): by default it is the organisation's.
    DataAccessSet {
        private: bool,
    },
    /// The admin read a person's data (`what`: tasks, chats, files, memory).
    PersonDataRead {
        person: String,
        what: String,
        /// One task, conversation, file or memory, when one was opened.
        #[serde(default)]
        item: Option<String>,
    },
    /// Root deleted a person's data (admin console spec, Users › Delete):
    /// their tasks, runs, conversations, files, memory, schedules and inbox
    /// leave the appliance's state; their content leaves the store (the
    /// `Removed` before it names what), and their sign-ins the vault.
    PersonErased {
        person: String,
    },
    /// The owner set a person's roles in Settings; they replace the config's
    /// unless the identity provider names the person's roles.
    RolesSet {
        person: String,
        roles: Vec<String>,
    },
    /// A person changed their own settings (notifications, privacy); the
    /// keys given replace those before.
    PreferencesSet {
        person: String,
        prefs: Value,
    },
    /// A person deleted entries from a thread: views leave them out; the log keeps them.
    ChatHidden {
        person: String,
        thread: String,
        entries: Vec<String>,
    },
    /// The person's reaction to one of Magnus's messages in their thread
    /// (behavior spec, Per-user relationship): `great` ("that was great"),
    /// `annoying` ("that was annoying"), or none, taken back. The latest
    /// counts. It names the message, not its words.
    Reacted {
        person: String,
        thread: String,
        entry: String,
        #[serde(default)]
        reaction: Option<String>,
    },
    /// Magnus advised one thing and the person decided otherwise: its
    /// position and their decision, recorded by Magnus in a run of theirs
    /// (`disagreement`).
    DisagreementRecorded {
        id: String,
        person: String,
        run: String,
        #[serde(default)]
        position: String,
        #[serde(default)]
        decision: String,
    },
    /// How a disagreement turned out, recorded later.
    DisagreementOutcome {
        id: String,
        person: String,
        run: String,
        #[serde(default)]
        outcome: String,
    },
    /// A person branched a thread at an entry: a new thread starts with the
    /// conversation up to there, as it was then.
    ThreadBranched {
        person: String,
        thread: String,
        from: String,
        upto: String,
        #[serde(default)]
        entries: Vec<Value>,
        #[serde(default)]
        title: String,
    },
    /// A person connected one of the organisation's apps with their own
    /// account (connectors spec, Lifecycle and audit). The tokens are in
    /// the vault, never here; `account` is whom the app says they are.
    AppConnected {
        app: String,
        person: String,
        #[serde(default)]
        account: Option<String>,
    },
    /// A person's connection to an app ended: Disconnect, or the person
    /// removed. `at_provider`: the app revoked the sign-in itself; else
    /// `detail` says why not, and the tokens are deleted here all the same.
    AppDisconnected {
        app: String,
        person: String,
        at_provider: bool,
        detail: String,
    },
    /// A person signed in from an app. The token itself is never logged.
    SignedIn {
        person: String,
        session: String,
        client: String,
    },
    SignedOut {
        person: String,
        session: String,
    },
    /// A person's password changed: `how` is "settings" (they gave the
    /// current one) or "reset" (an emailed recovery link). Never the password.
    PasswordChanged {
        person: String,
        how: String,
    },
    /// The person opened the app's home: what the feed showed them
    /// then is seen, so the next open starts here (#367).
    HomeOpened {
        person: String,
    },
    /// A computer was paired and Cloudflare signed its certificate. The
    /// certificate and its key are not here; the key never was.
    DevicePaired {
        person: String,
        device: String,
        name: String,
    },
    /// A paired computer was revoked: its certificate no longer opens the
    /// public name, and its sessions were ended. `by` is who revoked it.
    DeviceRevoked {
        person: String,
        device: String,
        by: String,
    },
    /// The identity provider, or an admin, turned a person off: their apps
    /// answer no calls for them until they sign in again.
    PersonTurnedOff {
        person: String,
        why: String,
    },
    /// A recurring task a person set up (or accepted from the agent's proposal).
    ScheduleSet {
        id: String,
        person: String,
        #[serde(default)]
        text: String,
        /// Minutes between runs, or None with `daily_at`.
        #[serde(default)]
        every_minutes: Option<u32>,
        /// "HH:MM" for a daily run, in its person's time zone (the server's when they set none).
        #[serde(default)]
        daily_at: Option<String>,
        /// Each run's limits, when they are tighter than the appliance's.
        #[serde(default)]
        budget: Option<crate::config::Budget>,
        /// The chat card the person confirmed to create it.
        #[serde(default)]
        card: Option<String>,
        #[serde(default)]
        label: Option<Label>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        until: Option<chrono::DateTime<chrono::Utc>>,
        #[serde(default)]
        max_runs: Option<u32>,
        #[serde(default)]
        scope: Option<String>,
        #[serde(default)]
        thread: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quarantine_schema: Option<Value>,
    },
    ScheduleSkipped {
        id: String,
        task: String,
        reason: String,
    },
    /// A schedule is over: it reached its end or its run count, or one of its own runs ended it.
    ScheduleEnded {
        id: String,
        /// Why: the kernel's words, or what the run that ended it wrote (content, removed with that run).
        #[serde(default)]
        reason: String,
        /// The run that ended it, when one did.
        #[serde(default)]
        run: Option<String>,
    },
    /// A schedule stopped starting runs after too many failed in a row,
    /// until its person resumes it.
    SchedulePaused {
        id: String,
        reason: String,
        /// The task whose run waits on a person: the schedule resumes by
        /// itself once it no longer does. None: paused for failures,
        /// until its person resumes it.
        #[serde(default)]
        task: Option<String>,
        /// An admin's suspension: only an admin lifts it.
        #[serde(default)]
        admin: bool,
        /// How it was done: "button" (the app), "conversation" (chat), or
        /// "console" (the admin UI). None: automatic, or logged before #267.
        #[serde(default)]
        source: Option<String>,
        /// The chat run that did it, when by conversation.
        #[serde(default)]
        turn: Option<String>,
    },
    /// When a schedule runs and when it ends, changed by its person (or the
    /// owner or an admin); what each run does stays.
    ScheduleChanged {
        id: String,
        every_minutes: Option<u32>,
        daily_at: Option<String>,
        until: Option<chrono::DateTime<chrono::Utc>>,
        max_runs: Option<u32>,
        /// How it was done: "button", "conversation" or "console" (#267).
        #[serde(default)]
        source: Option<String>,
        /// The chat run that did it, when by conversation.
        #[serde(default)]
        turn: Option<String>,
    },
    ScheduleResumed {
        id: String,
        /// How it was done: "button", "conversation" or "console" (#267).
        #[serde(default)]
        source: Option<String>,
        /// The chat run that did it, when by conversation.
        #[serde(default)]
        turn: Option<String>,
    },
    /// The person declined a card proposed in chat (a schedule or a
    /// suggestion), with their reason (none in logs from before #253).
    CardDeclined {
        card: String,
        #[serde(default)]
        person: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// The person answered "Not now" to a card proposed in chat.
    CardDeferred {
        card: String,
        #[serde(default)]
        person: Option<String>,
    },
    /// The person accepted a suggestion with no timing: it started this task.
    CardAccepted {
        card: String,
        person: String,
        task: String,
    },
    ScheduleRemoved {
        id: String,
        /// How it was done: "button", "conversation" or "console" (#267).
        #[serde(default)]
        source: Option<String>,
        /// The chat run that did it, when by conversation.
        #[serde(default)]
        turn: Option<String>,
    },
    /// A person's standing authority over a category of their own work
    /// (behavior spec, Standing authority; #235): set when they chose
    /// "Handle things like this" on a task proposal, or accepted a grant
    /// card Magnus put to them. An edit sets it again with the same id.
    /// `cedar` is the policy the kernel wrote from these fields: what it
    /// covers in the tasks started under it.
    GrantSet {
        id: String,
        person: String,
        /// The category of work, in words (content).
        #[serde(default)]
        title: String,
        /// The apps it covers, by name.
        apps: Vec<String>,
        /// Exclusions the person lifted: `send`, `irreversible`.
        #[serde(default)]
        allow: Vec<String>,
        /// `day`, `week` or `month`: a rolling 1, 7 or 30 days.
        period: String,
        /// Model cost its tasks may spend in a period.
        max_usd: f64,
        /// The card it was set from, if any.
        #[serde(default)]
        card: Option<String>,
        cedar: String,
        /// The taint of the chat turn whose words its title is, when they
        /// were not the person's own.
        #[serde(default)]
        label: Option<Label>,
    },
    /// The person revoked a grant: it covers nothing from now on.
    GrantRevoked {
        id: String,
        person: String,
    },
    /// An action a policy asked about went ahead under a grant.
    GrantUsed {
        grant: String,
        task: String,
        run: String,
        call_id: String,
        tool: String,
    },
    /// Something the person or Magnus said they would do (behavior spec,
    /// the commitment ledger; #236), recognised by Magnus in a run of the
    /// person's: `who` (`person` or `magnus`) will do `what`, for `to`, by
    /// `by` (a date or a time). `quote` is the words it was recognised
    /// from, found verbatim in the person's own words of the run or in the
    /// result of a call it read (`source`). `label` is the run's taint.
    CommitmentRecorded {
        id: String,
        person: String,
        who: String,
        #[serde(default)]
        what: String,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        by: Option<String>,
        /// `{kind: conversation, thread, task}` or `{kind: app, app, tool, task, call}`.
        #[serde(default)]
        source: Value,
        #[serde(default)]
        quote: String,
        #[serde(default)]
        label: Label,
    },
    /// A commitment corrected (its words, for whom, by when) or its status
    /// changed (`open`, `done`, `released`), by the person or by Magnus.
    /// What is given replaces what it had; an empty `to` or `by` clears it.
    CommitmentChanged {
        id: String,
        person: String,
        #[serde(default)]
        what: Option<String>,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        by: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        note: Option<String>,
    },
    /// The person removed a commitment; the content of its events is
    /// deleted with a `removed` event that follows.
    CommitmentRemoved {
        id: String,
        person: String,
    },
    /// A due schedule could not start its task.
    ScheduleFailed {
        id: String,
        reason: String,
    },
    ScheduleFired {
        id: String,
        task: String,
    },
    /// The model new tasks run on, chosen by the owner.
    ModelSet {
        adapter: String,
        model: String,
    },
    /// Retired (spec rev 140: definitions are screened, `Tools`): a tool
    /// definition pinned by hash, `approved` once the owner reviewed its text.
    ToolPinned {
        tool: String,
        hash: String,
        description: String,
        input_schema: Value,
        #[serde(default)]
        approved: bool,
    },
    /// Retired as `ToolPinned` is: a definition changed, awaiting review.
    ToolChanged {
        tool: String,
        hash: String,
        description: String,
        input_schema: Value,
        item: String,
    },
    TaskCreated {
        task: String,
        run: String,
        person: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        attachments: Vec<Value>,
        config: RunConfig,
        #[serde(default)]
        experiment: Option<ExperimentTag>,
        /// Where in chat it comes from: a chat turn, or a task started there.
        #[serde(default)]
        chat: Option<ChatOrigin>,
        /// Starting taint (from chat turn/proposal; person's own words carry none).
        #[serde(default)]
        label: Option<Label>,
        #[serde(default)]
        explicit_label: bool,
        /// A short title the model wrote when it started the task.
        #[serde(default)]
        title: Option<String>,
        /// Library files the person gave this task or message (uploads).
        #[serde(default)]
        files: Vec<String>,
        /// The project it started in.
        #[serde(default)]
        project: Option<ProjectContext>,
        /// The scope the person confirmed for the schedule that started it.
        #[serde(default)]
        scope: Option<String>,
        /// The grant it runs under (#235): its policy asks within the
        /// grant go ahead without asking.
        #[serde(default)]
        grant: Option<String>,
    },
    /// A file the person added: to the library, a project, or a chat
    /// message. Its content is private data, never instructions.
    FileUploaded {
        id: String,
        person: String,
        #[serde(default)]
        name: String,
        media_type: String,
        #[serde(default)]
        data_b64: String,
        /// Where it was added, when that was a chat thread.
        #[serde(default)]
        thread: Option<String>,
    },
    /// A project was made or changed by its person: its name and the
    /// instructions that apply to everything in it. Only the person writes
    /// these (I1); no tool does.
    ProjectSet {
        id: String,
        person: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        instructions: Option<String>,
    },
    /// The person moved a task, chat thread, schedule or file into a
    /// project, or out of one (`project` None).
    Moved {
        item: String,
        #[serde(default)]
        project: Option<String>,
    },
    ModelCall {
        run: String,
        step: u32,
        attempt: u32,
        context: ContextRef,
    },
    ModelReply {
        run: String,
        step: u32,
        #[serde(default)]
        blocks: Vec<Block>,
        #[serde(default)]
        state: Option<Value>,
        usage: Usage,
        served_model: String,
        stop: String,
    },
    ModelError {
        run: String,
        step: u32,
        attempt: u32,
        class: ErrorClass,
        message: String,
        /// What the provider charged before the call failed (it counts
        /// against the budget like a reply's).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// Text the loop shows the model after a step (a malformed reply, a
    /// verification failure without a tool call to answer).
    Notice {
        run: String,
        step: u32,
        text: String,
    },
    /// Recorded before an external call (intent first).
    Intent {
        run: String,
        call_id: String,
        step_id: String,
        key: String,
        tool: String,
        #[serde(default)]
        input: Value,
        issuer: String,
    },
    /// A boundary decision, with the stage and rule that produced it (E4).
    Decision {
        run: String,
        call_id: String,
        tool: String,
        outcome: String,
        stage: String,
        reason: String,
        #[serde(default)]
        rule: Option<String>,
    },
    /// The result of a tool call as the model will see it, already redacted.
    ToolResult {
        run: String,
        call_id: String,
        #[serde(default)]
        key: Option<String>,
        status: String,
        #[serde(default)]
        text: String,
        is_error: bool,
        label: Label,
        #[serde(default)]
        data: Option<Value>,
        /// The apps the content came from, for the audience rule.
        #[serde(default)]
        sources: Vec<String>,
        /// The result of a contract check: compared by the kernel, never
        /// given to the model as content, so it taints nothing.
        #[serde(default)]
        check: bool,
    },
    Contract {
        run: String,
        version: u32,
        #[serde(default)]
        contract: Value,
        /// A person's approval of a change to a frozen contract (retired, rev 147).
        #[serde(default)]
        approved_by: Option<String>,
    },
    /// Retired with contracts (spec rev 147): read in old logs only.
    WorkStarted {
        run: String,
    },
    /// Retired with contracts (spec rev 147): read in old logs only.
    ContractFrozen {
        run: String,
        version: u32,
    },
    /// The agent's plan (K36): ordered units, each with an id, a goal and
    /// when it is done. A revision is a new version.
    Plan {
        run: String,
        version: u32,
        #[serde(default)]
        plan: Value,
    },
    Verification {
        run: String,
        #[serde(default)]
        criterion: String,
        passed: bool,
        #[serde(default)]
        evidence: Value,
        by: String,
    },
    Suspended {
        run: String,
        reason: String,
        #[serde(default)]
        item: Option<String>,
    },
    Resumed {
        run: String,
    },
    Completed {
        run: String,
        #[serde(default)]
        summary: String,
        /// Fixed components the agent composed to show the outcome (U8).
        #[serde(default)]
        view: Option<Value>,
        /// The outcome written for listening, read aloud on request (U8).
        #[serde(default)]
        spoken: Option<String>,
    },
    Failed {
        run: String,
        #[serde(default)]
        reason: String,
        by: String,
    },
    Cancelled {
        run: String,
        /// How it was done: "button", "conversation" or "console" (#267).
        /// None: automatic, or logged before then.
        #[serde(default)]
        source: Option<String>,
        /// The chat run that did it, when by conversation.
        #[serde(default)]
        turn: Option<String>,
    },
    Item {
        id: String,
        item_kind: String,
        task: Option<String>,
        run: Option<String>,
        /// The person the run acts for, or the owner for system items.
        person: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        details: Value,
    },
    ItemResolved {
        id: String,
        decision: String,
        #[serde(default)]
        scope: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },
    /// The app presented an inbox item to its person (#367): when it
    /// was first seen, for opened-tracking.
    ItemOpened {
        id: String,
    },
    /// A watched read answered: these item ids were seen for the
    /// person's app tool, becoming the newest of its two-answer
    /// window (#369, #409). `full` tells whole answers from history:
    /// events before #411 hold fresh-only ids after the seed, and
    /// replay unions those onto the newest entry instead (#415).
    WatchedSeen {
        person: String,
        app: String,
        tool: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        full: bool,
    },
    /// A scheduled poll of a watched read started (#406): the poll
    /// clock for the person's app tool restarts here.
    WatchPolled {
        person: String,
        app: String,
        tool: String,
    },
    Approval {
        id: String,
        item: String,
        person: String,
        task: String,
        tool: String,
        #[serde(default)]
        input_hash: Option<String>,
        scope: String,
        #[serde(default)]
        until: Option<chrono::DateTime<chrono::Utc>>,
        #[serde(default)]
        reason: Option<String>,
        /// Kept here, as the item goes with its task: the one risk class it covers and (rev 140) the destinations it covers. Absent before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        risk: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destinations: Option<Vec<String>>,
    },
    ApprovalUsed {
        id: String,
        run: String,
        call_id: String,
    },
    Wait {
        run: String,
        until: chrono::DateTime<chrono::Utc>,
    },
    BudgetExtended {
        run: String,
        budget: crate::config::Budget,
    },
    /// A file the agent delivered to the person from its workspace (K19).
    Output {
        id: String,
        run: String,
        #[serde(default)]
        name: String,
        media_type: String,
        #[serde(default)]
        data_b64: String,
    },
    /// A conformance suite run for an adapter version.
    AdapterVerified {
        adapter_id: String,
        adapter: String,
        version: String,
        model: String,
        passed: bool,
        report: Value,
    },
    /// One itemized change to the agent's playbook (I2), made through the
    /// boundary like a memory write and labeled with the writing run's taint.
    Playbook {
        item: String,
        op: String,
        person: String,
        run: String,
        #[serde(default)]
        text: Option<String>,
        /// The failure the item addresses.
        #[serde(default)]
        addresses: Option<String>,
        label: Label,
    },
    /// The person's verdict on an outcome (a task's, or a chat answer's):
    /// right, partly or wrong, with an optional note. The latest counts.
    Verdict {
        task: String,
        person: String,
        verdict: String,
        #[serde(default)]
        note: Option<String>,
    },
    /// A grader's verdict on a run, recorded by the evaluation runner (V2).
    Graded {
        run: String,
        grader: String,
        passed: bool,
        #[serde(default)]
        score: Option<f64>,
        #[serde(default)]
        details: Value,
    },
    Memory {
        id: String,
        person: String,
        run: String,
        #[serde(default)]
        fact: String,
        #[serde(default)]
        topic: Option<String>,
        /// The writing run's taint; reading the entry back carries it (M1).
        label: Label,
        #[serde(default)]
        sources: Vec<String>,
        /// Who filed it: the run's model (None) or the memory pass.
        #[serde(default)]
        by: Option<String>,
    },
    /// One model call of the memory pass after a chat turn or a run ended.
    MemoryPass {
        run: String,
        model: String,
        #[serde(default)]
        blocks: Vec<magnus_adapter_proto::Block>,
        usage: magnus_adapter_proto::Usage,
    },
    /// An entry joins the company knowledge base (M1-M4). `readers` are
    /// clauses that must all be met, each by any one of its principals
    /// (`person:<id>`, `group:<id>`); none means everyone. `integrity` is
    /// `company`, `verified` or `untrusted`; `source` keeps where it came
    /// from (citations, file, app, or the entry a value was extracted from).
    /// A version of a document in the org context bundle (OKF v0.2, the
    /// Magnus profile; `bundle.rs`). Its frontmatter and body are content:
    /// the log keeps what the kernel needs to find and guard it.
    BundleWritten {
        path: String,
        version: u32,
        doc_kind: String,
        /// `org`, or `user` for a person's own document under `/users/<owner>/`.
        scope: String,
        #[serde(default)]
        owner_user: Option<String>,
        /// The OKF actor that wrote it (`magnus/<version>`, `human:<id>`).
        actor: String,
        #[serde(default)]
        run: Option<String>,
        /// The taint of what it was written from (M1): read back with it.
        label: Label,
        /// The items it lists in `sources` (their `resource`s): who may see
        /// it follows from who may read these (K2).
        #[serde(default)]
        sources: Vec<String>,
        /// The person whose run read its sources (K2): an agent's version
        /// names its run's, a version put back the one it was written
        /// with; an admin's edit names none and keeps the document's.
        #[serde(default)]
        writer: Option<String>,
        #[serde(default)]
        frontmatter: Value,
        #[serde(default)]
        body: String,
    },
    /// An admin removed an org document from the bundle (admin console
    /// spec, Knowledge). Its versions stay in the log, to roll back to.
    BundleRemoved {
        path: String,
    },
    KnowledgeAdded {
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        text: String,
        readers: Vec<Vec<String>>,
        integrity: String,
        /// What it came from, with its citations' quotes: content.
        #[serde(default)]
        source: Value,
        author: String,
        run: Option<String>,
        /// How the kernel checked its citations, with the evidence.
        #[serde(default)]
        verification: Option<Value>,
        /// A typed value (number, boolean or one of named options).
        #[serde(default)]
        value: Option<Value>,
    },
    /// A person with the role made an entry a company fact, from an inbox
    /// card; its source stays attached.
    KnowledgeEndorsed {
        id: String,
        by: String,
        item: Option<String>,
        note: Option<String>,
    },
    /// A reader widened who may read an entry (share, #299): the new
    /// readers, and who shared it.
    KnowledgeShared {
        id: String,
        readers: Vec<Vec<String>>,
        by: String,
    },
    /// The writer of an org document shared it (#299): principals added
    /// to who may see it, or everyone, and who shared it.
    BundleShared {
        path: String,
        #[serde(default)]
        add: Vec<String>,
        #[serde(default)]
        everyone: bool,
        by: String,
    },
    /// One model call with no tools that read an entry's sources for a typed value.
    KnowledgeExtraction {
        run: String,
        model: String,
        #[serde(default)]
        reply: String,
        usage: magnus_adapter_proto::Usage,
    },
    /// An answer written for listening, by a model call with no tools, when
    /// the person first asked to hear it.
    SpokenWritten {
        run: String,
        model: String,
        #[serde(default)]
        script: String,
        usage: magnus_adapter_proto::Usage,
        /// The character the script was rewritten as (#269).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        character: Option<String>,
    },
    MemoryForgotten {
        id: String,
    },
    Trashed {
        item: String,
        person: String,
    },
    Restored {
        item: String,
    },
    Removed {
        item: String,
        seqs: Vec<i64>,
    },
    QueryGateRecorded(QueryGatePayload),
    SafetyBlocked(SafetyBlockedPayload),
    TeamMission(TeamMissionEvent),
    MemorySuperseded(MemorySupersededPayload),
    MemoryArchived(MemoryArchivedPayload),
    PrefetchRecorded {
        run: String,
        person: String,
        #[serde(default)]
        pointers: String,
        #[serde(default)]
        label: Label,
    },
}

/// What each kind of event holds that a person or a source wrote (stored in `public.content`).
pub const CONTENT: &[(&str, &[&str])] = &[
    ("task_created", &["text", "attachments", "title", "chat.conversation", "project.name", "project.instructions", "scope"]),
    ("file_uploaded", &["name", "data_b64"]),
    ("project_set", &["name", "instructions"]),
    ("thread_branched", &["entries", "title"]),
    ("schedule_set", &["text", "title", "scope"]),
    ("model_reply", &["blocks", "state"]),
    ("verifier_reply", &["blocks"]),
    ("memory_pass", &["blocks"]),
    ("intent", &["input"]),
    ("tool_result", &["text", "data"]),
    ("handback", &["image_b64"]),
    ("contract", &["contract"]),
    ("plan", &["plan"]),
    ("verification", &["criterion", "evidence"]),
    ("evidence", &["found", "sources", "note"]),
    ("completed", &["summary", "view", "spoken"]),
    ("failed", &["reason"]),
    ("item", &["summary", "details"]),
    ("item_resolved", &["text"]),
    ("output", &["name", "data_b64"]),
    ("playbook", &["text", "addresses"]),
    ("verdict", &["note"]),
    ("graded", &["details"]),
    ("memory", &["fact", "topic"]),
    ("knowledge_added", &["title", "text", "source", "verification", "value"]),
    ("bundle_written", &["frontmatter", "body"]),
    ("knowledge_extraction", &["reply"]),
    ("spoken_written", &["script"]),
    ("disagreement_recorded", &["position", "decision"]),
    ("disagreement_outcome", &["outcome"]),
    ("card_declined", &["reason"]),
    ("schedule_ended", &["reason"]),
    ("draft_scoped", &["participants"]),
    ("grant_set", &["title"]),
    ("commitment_recorded", &["what", "to", "quote"]),
    ("commitment_changed", &["what", "to", "note"]),
    ("watched_seen", &["ids"]),
    ("rule", &["words"]),
    ("spend", &["reason", "note"]),
    ("team_mission", &["goal", "objective", "draft", "contribution", "markdown"]),
    ("team", &["name", "message", "history", "file", "text", "note", "def", "entry", "routine", "chat"]),
    ("prefetch_recorded", &["pointers"]),
];

/// The content fields of one kind of event (a lane stored under its lane's kind, a mission's); none for most.
pub fn content_fields(kind: &str) -> &'static [&'static str] {
    CONTENT.iter().find(|(k, _)| *k == kind || (*k == "team_mission" && OLD_LANE_KINDS.contains(&kind))).map(|(_, f)| *f).unwrap_or(&[])
}

#[cfg(test)]
#[path = "events_replay_test.rs"]
mod replay_test;

#[cfg(test)]
#[path = "events_content_test.rs"]
mod content_test;
