# Proactive Semantic Outreach Design

## Goal

Build a bot that can proactively reach out to the user without waiting for a new inbound message.

Scope for the first version:

- The bot predicts what the user is likely to do next from recent conversation context.
- The bot sends a proactive suggestion or permission-seeking question.
- The bot does not execute actions automatically.
- Prediction is based mainly on current session semantics, not external systems or long-term user profiling.

## Product Boundary

Desired behavior:

- If the recent conversation strongly implies a likely next step, the bot may ask whether it should help with that step.
- The proactive message should be concrete and actionable.
- The bot should be quiet when confidence is low.

Out of scope for v1:

- Auto-execution without consent
- Broad external event ingestion
- Heavy long-term memory across many sessions
- Generic "need help?" nudges with no specific action

## Recommended Approach

Use a `semantic state + policy + LLM decision` architecture instead of pure cron or pure prompt-only behavior.

Why:

- More proactive than `heartbeat` and `cron`
- More controllable than a raw prompt-only predictor
- Fits the existing app/background/session structure in this repository

## Proposed Components

### 1. `ContextSummarizer`

Input:

- Recent messages from the current session

Output:

- Current user goal
- Unfinished task
- Likely next action
- Confidence hints
- Whether the user appears blocked, waiting, or about to continue

Notes:

- This can be LLM-generated structured output.
- Keep it short and cheap so it can run periodically.

### 2. `IntentPredictor`

Input:

- Session summary from `ContextSummarizer`

Output:

- Top candidate next actions
- Confidence per candidate
- Suggested outreach wording seed

Rules:

- Prefer ranked candidates over a single guess.
- Require a minimum confidence threshold before outreach.

### 3. `ProactivityPolicy`

Input:

- Predictor output
- Last user activity time
- Last proactive outreach time
- Recent accept/reject/ignore outcomes

Output:

- `NoAction`
- `Wait`
- `AskUser { message }`

Responsibilities:

- Enforce cooldowns
- Prevent duplicate nudges
- Block low-value or low-confidence outreach
- Prefer specific asks over vague offers

### 4. `ProactiveDispatcher`

Responsibilities:

- Scan eligible sessions in the background
- Run summarization and prediction
- Publish outbound proactive messages when policy allows

This should be separate from normal inbound handling so the bot can speak first.

## Integration Points In This Repo

Most natural attachment points:

- [crates/app/src/background.rs](/data00/home/lujianhui.1/nanobot-rs/crates/app/src/background.rs)
  - add a periodic proactive scan step to the background worker
- [crates/app/src/app.rs](/data00/home/lujianhui.1/nanobot-rs/crates/app/src/app.rs)
  - add an app-level proactive outreach flow that can publish outbound messages without a new inbound trigger
- [crates/session/src/manager.rs](/data00/home/lujianhui.1/nanobot-rs/crates/session/src/manager.rs)
  - read recent session history as predictor input
- [crates/core/src/agent_loop.rs](/data00/home/lujianhui.1/nanobot-rs/crates/core/src/agent_loop.rs)
  - avoid mixing proactivity policy directly into the normal assistant loop; keep policy outside the main turn execution path

## Suggested Runtime Flow

1. Background worker wakes up.
2. App selects sessions that are eligible for proactive evaluation.
3. For each eligible session, load recent conversation history.
4. Generate a compact semantic summary.
5. Predict likely next user action.
6. Run proactivity policy.
7. If allowed, publish one proactive outbound message.
8. Record outreach metadata for cooldown and deduplication.

## Guardrails

These are mandatory for usefulness.

- Cooldown per session
- Deduplication of repeated suggestions
- Minimum confidence threshold
- Only send concrete questions
- Silence after repeated rejection or ignore
- Cap the number of proactive messages in a time window

Examples of good outreach:

- "You were probably about to continue debugging this error. Want me to draft a step-by-step diagnosis path?"
- "It looks like the next step is turning this idea into an implementation plan. Want me to break it down now?"

Examples of bad outreach:

- "Need any help?"
- "Just checking in"
- Repeating the same guess every few minutes

## Data Model Additions

Likely needed session metadata:

- `last_user_message_at`
- `last_proactive_at`
- `last_proactive_fingerprint`
- `proactive_accept_count`
- `proactive_reject_count`
- `proactive_ignore_count`

These can start in session metadata before introducing a separate store.

## Implementation Order

Phase 1:

- Add session eligibility scan in the background worker
- Add a simple summary + predictor interface
- Add app-level proactive outbound publishing
- Add cooldown and deduplication

Phase 2:

- Improve predictor prompt/schema
- Add feedback-aware suppression
- Add better selection of eligible sessions

Phase 3:

- Consider multi-session memory or user-level preference modeling if v1 quality is good

## Recommendation

Start with current-session semantic prediction only.

Do not begin with:

- cross-session user profiling
- direct auto-execution
- external signal fusion

The first success criterion should be:

- the bot occasionally sends a clearly relevant, specific, low-frequency question that the user actually wants to answer

## Open Questions For Later

- How long should the silence window be before proactive outreach is allowed?
- Should different channels have different proactivity limits?
- How should "ignored" vs "rejected" be detected?
- Should proactive messages use the same model/provider path as normal chat turns?
