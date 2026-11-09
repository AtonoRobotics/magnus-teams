//! Every kind of event the log holds. The body of each row is one `Ev`.
//! Secrets never appear here: a secret write records only its name.

use crate::tools::Label;
use magnus_adapter_proto::{Block, ErrorClass, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A run's place in a chat thread.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatOrigin {
    pub thread: String,
    /// A chat turn (the person's message, answered without a contract), or a
    /// task started from chat.
    pub turn: bool,
    /// For a task: the chat turn that started it.
    #[serde(default)]
    pub from_run: Option<String>,
    /// For a task: the conversation that led to it, carried into its contract.
    #[serde(default)]
    pub conversation: Vec<Value>,
    /// A turn Magnus started itself (behavior spec, cold start and a newly
    /// connected app): its text is the appliance's instruction, never the
    /// person's message. Absent in logs from before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opener: Option<Box<Opener>>,
}

/// Why Magnus opened a conversation itself: the person's cold start (no
/// app), or their first connection of `app`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Opener {
    /// The app's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The app as the person sees it named, when it was connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_title: Option<String>,
}

/// The project a task started in, as it stood then: the person's
/// instructions for everything in it and its reference files. Fixed at the
/// start, so the task's context only grows (C1, K5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProjectContext {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub instructions: Option<String>,
    /// Library file ids, read on demand with `project_read` (M2).
    #[serde(default)]
    pub files: Vec<String>,
}

/// A run's pinned configuration (K9), recorded at its start.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunConfig {
    pub adapter: String,
    pub model: String,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub budget: crate::config::Budget,
    pub verifier: bool,
    pub instructions: String,
    pub harness: String,
    /// The playbook version the run started with (I3).
    #[serde(default)]
    pub playbook: u64,
    /// Removable modules on or off for this run, pinned at its start (K9).
    #[serde(default)]
    pub modules: std::collections::BTreeMap<String, bool>,
    /// Where a lost run escalates to (`adapter/model`), if anywhere.
    #[serde(default)]
    pub escalate_to: Option<String>,
    #[serde(default)]
    pub lost: crate::run::lost::Thresholds,
    /// The verifier's model (`adapter/model`); the run's own when unset.
    #[serde(default)]
    pub verifier_model: Option<String>,
}

impl RunConfig {
    /// A removable module's switch for this run. Runs from before modules
    /// were pinned had none of them, except the verifier's flag.
    pub fn module(&self, name: &str) -> bool {
        match self.modules.get(name) {
            Some(v) => *v,
            None => name == "verifier" && self.verifier,
        }
    }
}

/// A task started as one trial of an experiment case (V2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExperimentTag {
    pub id: String,
    pub case: String,
    pub trial: u32,
}

/// How to rebuild exactly what one model call was sent (K5): the run's
/// events up to `through_seq`, with these instructions and tools.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContextRef {
    pub through_seq: i64,
    pub instructions: String,
    pub tools_hash: String,
    /// The version of the person's relationship notes the call carried in
    /// its system context (`relationship.rs`); none when they have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<u32>,
    /// Whether the call carried anything of its person's (notes, recent
    /// messages, reactions, commitments): what the run then writes for
    /// others is theirs (#258, #301).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub personal: bool,
    /// The character the reply spoke as, so alpha feedback traces to it
    /// (#269). None on calls logged before it: they spoke with it too,
    /// but only inside the hashed instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
}

impl ContextRef {
    /// Whether the call carried anything of its person's. A call logged
    /// before `personal` was recorded says so by the notes version it
    /// carried (#304).
    pub fn carried_person(&self) -> bool {
        self.personal || self.relationship.is_some()
    }
}

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
    /// An admin turned a run module on or off for the organisation (None:
    /// back to the config's), for runs that start from then.
    ModuleSet {
        name: String,
        on: Option<bool>,
    },
    /// An admin enabled or disabled a model provider (an adapter).
    ProviderSet {
        id: String,
        enabled: bool,
    },
    /// The release channel the desktop apps of everyone but the owner and
    /// testers follow.
    DesktopChannelSet {
        channel: String,
    },
    /// The admin console's own session lifetimes, as an admin set them
    /// under Security › Sign-in.
    ConsoleSettings {
        idle_minutes: i64,
        session_hours: i64,
        /// The subnets it takes connections from, as an admin set them
        /// under Operations › Network (None: the config's).
        #[serde(default)]
        allow: Option<Vec<String>>,
    },
    /// An admin's draft of a policy layer (None: dropped). Nothing in force
    /// changes until it is applied.
    PolicyDrafted {
        layer: String,
        cedar: Option<String>,
    },
    KillSwitch {
        engaged: bool,
    },
    /// A computer changed (`computer::Change`).
    Computer(crate::computer::Change),
    /// Something done in the admin console (admin console spec, Authority
    /// and audit): who (the admin's `sub` and name), what, to what, from
    /// where, and for a change, what it was before and after. Written by the
    /// kernel at the console's request, before the console answers.
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
    /// The lost-reset module found a no-progress signal and sent the run back
    /// to the reply where the loop began, with a note; the replies from
    /// `from` to `to` leave the model's context (they stay in the log).
    ContextReset {
        run: String,
        from: u32,
        to: u32,
        signal: String,
        note: String,
    },
    /// One model call of the verifier module: reviewing the contract's
    /// criteria before work (`criteria`), or the work against them (`work`).
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
    /// The owner classified one of an app's tools (None: not offered).
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
    /// A person deleted entries from a thread: views leave them out; the
    /// log keeps them.
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
        /// Local time "HH:MM" for a daily run.
        #[serde(default)]
        daily_at: Option<String>,
        /// Each run's limits, when they are tighter than the appliance's.
        #[serde(default)]
        budget: Option<crate::config::Budget>,
        /// The chat card the person confirmed to create it.
        #[serde(default)]
        card: Option<String>,
        /// The taint of the chat turn that proposed it; its tasks start with it.
        #[serde(default)]
        label: Option<Label>,
        /// A short title the model wrote for it.
        #[serde(default)]
        title: Option<String>,
        /// No run starts at or after this time.
        #[serde(default)]
        until: Option<chrono::DateTime<chrono::Utc>>,
        /// No run starts after this many.
        #[serde(default)]
        max_runs: Option<u32>,
        /// What the person confirmed each run does, on the chat card.
        #[serde(default)]
        scope: Option<String>,
    },
    /// A due schedule started no run because its last one is still open:
    /// running, or waiting on someone. The next is due as usual.
    ScheduleSkipped {
        id: String,
        task: String,
        reason: String,
    },
    /// A schedule is over: it reached its end or its run count, or one of
    /// its own runs ended it.
    ScheduleEnded {
        id: String,
        /// Why: the kernel's words, or what the run that ended it wrote
        /// (content, removed with that run).
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
    /// A tool definition approved for use, pinned by hash (B10).
    ToolPinned {
        tool: String,
        hash: String,
        description: String,
        input_schema: Value,
        /// Whether the owner has reviewed this definition's text. Unreviewed
        /// descriptions are third-party text and never reach the model.
        #[serde(default)]
        approved: bool,
    },
    /// A tool's definition no longer matches its pin; it is disabled.
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
        /// Taint the run starts with: that of the chat turn or proposal that
        /// started it (the person's own words carry none).
        #[serde(default)]
        label: Option<Label>,
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
        /// Set when a person approved this version as a change to a frozen
        /// contract.
        #[serde(default)]
        approved_by: Option<String>,
    },
    /// The agent's first action that changes anything: from here on the
    /// contract in force is frozen.
    WorkStarted {
        run: String,
    },
    /// The contract at this version is frozen: a later change needs a
    /// person's approval, and only frozen or approved criteria count.
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
        /// The risk class the action had when approved: the approval covers
        /// only that class. Kept here, not only in the item, whose details
        /// are deleted with the task. Absent from approvals written before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        risk: Option<String>,
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
    /// The person had a memory forgotten; it is never read back again.
    MemoryForgotten {
        id: String,
    },
    /// An item moved to the trash by `person`: out of every view, restorable
    /// until it is removed. A task, a conversation, one message or answer in
    /// one (`m:<task>`, `a:<task>`), a file, a project, a memory or a
    /// knowledge entry.
    Trashed {
        item: String,
        person: String,
    },
    Restored {
        item: String,
    },
    /// A trashed item removed: the content of the events `seqs` was deleted
    /// from the store. The events stay, as the record.
    Removed {
        item: String,
        seqs: Vec<i64>,
    },
}

/// What each kind of event holds that a person or a source wrote. It is kept
/// beside the log in the content store, where it can be deleted; the log
/// keeps the rest. A dotted name is a field of a field.
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
    ("contract", &["contract"]),
    ("plan", &["plan"]),
    ("verification", &["criterion", "evidence"]),
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
];

/// The content fields of one kind of event; none for most.
pub fn content_fields(kind: &str) -> &'static [&'static str] {
    CONTENT.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f).unwrap_or(&[])
}

impl Ev {
    /// The version a document was written as (0 for any other event).
    pub fn bundle_version(&self) -> u32 {
        match self {
            Ev::BundleWritten { version, .. } => *version,
            _ => 0,
        }
    }

    pub fn run(&self) -> Option<&str> {
        use Ev::*;
        match self {
            TaskCreated { run, .. }
            | ModelCall { run, .. }
            | ModelReply { run, .. }
            | ModelError { run, .. }
            | Notice { run, .. }
            | Intent { run, .. }
            | Decision { run, .. }
            | ToolResult { run, .. }
            | Contract { run, .. }
            | WorkStarted { run }
            | ContractFrozen { run, .. }
            | Plan { run, .. }
            | Playbook { run, .. }
            | Graded { run, .. }
            | Verification { run, .. }
            | ContextReset { run, .. }
            | Escalated { run, .. }
            | VerifierReply { run, .. }
            | ReviewerReply { run, .. }
            | Suspended { run, .. }
            | Resumed { run }
            | Completed { run, .. }
            | Failed { run, .. }
            | Cancelled { run, .. }
            | ApprovalUsed { run, .. }
            | GrantUsed { run, .. }
            | Wait { run, .. }
            | BudgetExtended { run, .. }
            | Memory { run, .. }
            | MemoryPass { run, .. }
            | SpokenWritten { run, .. }
            | KnowledgeExtraction { run, .. }
            | DraftScoped { run, .. }
            | ReviewWoken { run, .. }
            | Output { run, .. } => Some(run),
            Item { run, .. } | ScheduleEnded { run, .. } => run.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    /// A call logged before `personal` was recorded, carrying the person's
    /// notes, still counts as carrying them when replayed (#304).
    #[test]
    fn a_call_logged_before_personal_with_notes_carried_its_person() {
        let old: super::ContextRef = serde_json::from_value(serde_json::json!({ "through_seq": 7, "instructions": "v1", "tools_hash": "h", "relationship": 3 })).unwrap();
        assert!(!old.personal);
        assert!(old.carried_person());
        let none: super::ContextRef = serde_json::from_value(serde_json::json!({ "through_seq": 7, "instructions": "v1", "tools_hash": "h" })).unwrap();
        assert!(!none.carried_person());
    }
}

#[cfg(test)]
#[path = "events_content_test.rs"]
mod content_test;
