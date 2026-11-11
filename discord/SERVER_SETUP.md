# NOCTURN — Discord server setup

The source of truth for the official NOCTURN Discord server: structure, roles,
permissions, and every posted message. Messages are written to fit Discord's
2,000-character limit and use only Discord markdown.

Brand notes (from `src/styles/global.css` and `src/shell/theme.ts`):
moonlight silver `#DFE2E8` / `#AEB3BE` on deep charcoal `#08080A`–`#20232A`,
with the muted violet `#A98BCC` from the Nebula theme as the one colour accent.
Wordmark is **NOCTURN** in capitals. Emoji only on category names and role
names, nowhere else.

---

## 1. Server settings

| Setting | Value |
|---|---|
| Name | NOCTURN |
| Icon | `src-tauri/icons/Square310x310Logo.png` (crescent mark) |
| Community | Enabled (needed for onboarding, rules screening, forums, announcement channel) |
| Verification level | Medium — verified email and registered on Discord for 5+ minutes |
| Explicit media filter | Scan media from all members |
| Default notifications | Only @mentions |
| Rules channel | #rules |
| Community updates channel | #moderation-log |
| Safety alerts channel | #moderation-log |
| System messages | Off (no join messages in #general; keeps it clean) |

## 2. Roles (top to bottom)

| Role | Colour | Shown separately | Server-level permissions |
|---|---|---|---|
| 👑 Founder | `#E8EAEE` | Yes | Manage Server, Manage Roles, Manage Channels, View Audit Log, Manage Webhooks, Manage Expressions, Manage Events, Kick, Ban, Timeout, Manage Messages, Manage Threads, Mention @everyone. **No Administrator** — the server owner already has every permission. |
| 🌙 Nocturn Team | `#A98BCC` | Yes | Same as Founder. No Administrator. |
| 🛡️ Moderator | `#8D95A8` | Yes | Kick, Ban, Timeout members, Manage Messages, Manage Threads, Manage Nicknames, View Audit Log, Mute / Deafen / Move members. No Manage Server, Roles, Channels or Webhooks. |
| 🧪 Tester | `#9C8FC4` | Yes | None extra — access comes from the Testing Lab channel permissions. |
| ✨ Contributor | `#C4C8D1` | Yes | None extra — recognition only. |
| 👤 Member | default | No | Embed Links, Attach Files, Add Reactions, Use External Emoji, Read Message History, Use Application Commands, Connect, Speak, Video, Use Voice Activity, Change Nickname, Create Invite. **Send Messages / Send in Threads / Create Public Threads are granted per category** (COMMUNITY, SUPPORT, FEEDBACK), so read-only channels stay read-only without deny overrides that would also block staff. |
| 🌑 New Arrival | `#555A64` | No | None. |
| @everyone | — | — | View Channels, Read Message History, Add Reactions only. Cannot send messages anywhere. |

### How New Arrival works without a bot

Discord can't remove a role when someone finishes onboarding, so the
**permission logic for "New Arrival" lives on @everyone**: anyone who hasn't
completed onboarding has only @everyone and sees START HERE, read-only.
Finishing onboarding grants 👤 Member, which unlocks the server.

The 🌑 New Arrival role exists for display and for a future auto-role bot
(assign on join, remove when Member is added). It carries no permissions,
so it can never grant or block access by mistake.

## 3. Channels and permissions

Permissions are set on the **category** and each channel stays synced unless
noted. "Staff" = 🌙 Nocturn Team + 🛡️ Moderator (Founder is the owner and
sees everything).

### 🌙 START HERE — @everyone: view ✔, send ✘ · Staff: send ✔
| Channel | Type | Topic | Notes |
|---|---|---|---|
| #welcome | text | Start here. What NOCTURN is and how to get access. | |
| #rules | text (rules) | The NOCTURN community rules. Read before posting. | |
| #announcements | announcement | Official NOCTURN news — releases, major features, beta launches, maintenance. | Only 🌙 Nocturn Team can post (Moderator send ✘). Others can follow it into their own servers. |
| #roadmap | text | What we're working on, and what's next. Plans can change. | |
| #server-status | text | Service and download status. Check here first if something seems down. | |

### 💬 COMMUNITY — @everyone: view ✘ · 👤 Member: view ✔, send ✔
| Channel | Type | Topic |
|---|---|---|
| #general | text | General conversation about NOCTURN and PC performance. |
| #introductions | text | Say hello — your setup, what you use NOCTURN for. |
| #showcase | text | Share your setups, benchmarks and before/after results. Slowmode 30s. |
| #suggestions | text | Quick ideas and small improvements. Detailed proposals go in #feature-requests. |
| #off-topic | text | Anything else. Keep it friendly. |
| General Lounge | voice | — |

### 🛠️ NOCTURN SUPPORT — @everyone: view ✘ · 👤 Member: view ✔, send ✔
| Channel | Type | Topic | Notes |
|---|---|---|---|
| #support | **forum** | Get help with NOCTURN. One post per issue — use the template. | Tags: Open, Answered, Needs Info. Post guidelines = support template. |
| #known-issues | text | Issues we already know about, with workarounds. Read-only. | Member send ✘ |
| #faq | text | Frequently asked questions. Read-only. | Member send ✘ |
| #guides | text | How-tos for getting the most from NOCTURN. Read-only. | Member send ✘ |

### 🐛 FEEDBACK & DEVELOPMENT — @everyone: view ✘ · 👤 Member: view ✔, send ✔
| Channel | Type | Topic | Notes |
|---|---|---|---|
| #bug-reports | **forum** | Report bugs in released versions. One bug per post — use the template. | Tags (moderated, staff-only): Investigating, Confirmed, Fixed, Cannot Reproduce. |
| #feature-requests | **forum** | Propose new features. Vote with the reaction. Votes guide us but don't guarantee implementation. | Default reaction ▲ (`⬆️`). Tags: Under Review, Planned, In Progress, Shipped, Not Planned (staff-only). |
| #feedback | text | General thoughts on NOCTURN — what you like, what could be better. | |
| #changelog | text | Detailed version notes for every release. Read-only. | Member send ✘ |

### 🧪 TESTING LAB — @everyone: view ✘ · 🧪 Tester + Staff: view ✔, send ✔
| Channel | Type | Topic | Notes |
|---|---|---|---|
| #tester-announcements | text | Test cycles, builds to test, deadlines. Read-only. | Tester send ✘ |
| #beta-downloads | text | Pre-release builds. Confidential — do not share outside this channel. Read-only. | Tester send ✘ |
| #testing-feedback | text | Feedback on the current pre-release build. | |
| #testing-bugs | text | Bugs found in pre-release builds. Use the bug template. | |
| #testing-guidelines | text | How testing works here. Read before your first test. Read-only. | Tester send ✘ |

### 🔒 STAFF HQ — @everyone: view ✘ · Staff: view ✔, send ✔
| Channel | Type | Topic | Notes |
|---|---|---|---|
| #staff-chat | text | Staff coordination. | |
| #moderation-log | text | AutoMod alerts and moderation actions. Log every warning, timeout, kick and ban here. | |
| #development | text | Development discussion. | |
| #release-planning | text | Release scheduling and checklists. | |

Deny overrides in this category apply to @everyone **and** 👤 Member,
🧪 Tester, ✨ Contributor, 🌑 New Arrival are simply not granted — a role
without an allow can't see a channel whose @everyone view is denied.

## 4. Onboarding (Discord native)

1. **Rules screening** (Membership Screening): people must agree to the
   server rules before they can do anything.
2. **Onboarding**, default channels: #welcome, #rules, #announcements, plus
   #general, #support, #faq (these appear once Member is granted).
3. **One required question** — "Welcome to NOCTURN":
   - Option: *"I've read the rules — let me in"* → grants 👤 Member.
4. **Server guide** — welcome sign: *"The official community for NOCTURN —
   PC optimisation, diagnostics and troubleshooting for Windows."*
   To-dos: Read the rules (#rules) · Introduce yourself (#introductions) ·
   Need help? (#support). Resource pages: #faq, #known-issues, #guides.

Result: arrive with @everyone (New Arrival access) → read #welcome → accept
rules → answer the one question → 👤 Member → full community.

### Verification bot (replaces Discord Onboarding)

**Status: live (2026-11-11).** Carl-bot added with 7 permissions (no
Administrator), role placed directly under 🛡️ Moderator. Prompt posted in
#rules (message 1570099734914269275), ✅ → 👤 Member via `/reactionrole add`,
message set to verify mode via `/reactionrole modify choice:verify`.
#welcome step 2 now says "React with ✅ under the rules to accept them".
System join/boost/tip messages are off so #welcome stays clean.

Bot: **Carl-bot** (free, widely used). Steps used:

1. Go to carl.gg → Invite. On Discord's authorise screen pick **NOCTURN** and
   untick Administrator. Keep only: View Channels, Manage Roles, Send Messages,
   Embed Links, Add Reactions, Read Message History, Use External Emoji.
2. Server Settings → Roles: drag the Carl-bot role **above 👤 Member** and
   below 🛡️ Moderator (a bot can only assign roles below its own).
3. Post the accept prompt in #rules (staff can post there):
   `React with ✅ to accept the rules and unlock the server.`
4. In #moderation-log run `/reactionrole add message_id:<ID> emoji:✅
   role:@👤 Member channel:#rules`, then `/reactionrole modify choice:verify
   message_id:<ID>` so the role can't be removed by un-reacting.
5. Test with View Server as Role (New Arrival) and with an alt account.

New Arrival stays "no roles" (the @everyone baseline); the 🌑 New Arrival role
remains a label only, so no stale role is left behind after verification.

## 5. AutoMod (native, free)

| Rule | Action |
|---|---|
| Block mention spam — more than 5 unique mentions | Block message, alert #moderation-log, timeout 10 minutes |
| Block suspected spam content | Block message, alert #moderation-log |
| Block commonly flagged words (all three lists) | Block message, alert #moderation-log |
| Suspicious links (custom keyword) — `*free nitro*`, `*nitro gift*`, `*discord-gift*`, `*discordgift*`, `*dlscord*`, `*disc0rd*`, `*steamcommunlty*`, `*stearncommunity*`, `*airdrop*`, `*grabify*`, `*iplogger*` | Block message, alert #moderation-log |
| Invite links (custom regex) — `(discord\.gg|discord(app)?\.com/invite)/` | Block message, alert #moderation-log. Exempt: Staff. |

Exempt from all rules: 🌙 Nocturn Team, 🛡️ Moderator.

---

## Build status (2026-11-11)

Built in the existing empty "NOCTURN" server (the two default channels were
reused: Text channels → 🌙 START HERE with #general renamed #welcome; Voice
channels → 💬 COMMUNITY with General renamed General Lounge).

- Done: roles + colours + permissions, @everyone limited, all 6 categories and
  29 channels, topics, forum guidelines/tags/⬆️ reaction, read-only channels,
  AutoMod (4 rules → #moderation-log), sensitive-media filter (all members).
- Posted: #welcome, #rules, #faq, #testing-guidelines, #moderation-log procedure.
- Not posted (need owner review — content would make claims about status and
  plans): #roadmap, #server-status, #guides, #known-issues. Copy is below.
- Community enabled (owner-approved): verified email required, mentions-only
  notifications, #rules = rules channel, #moderation-log = updates + safety
  alerts, #announcements converted to an Announcement channel (followable).
- Discord Onboarding is **not used**: it requires 7 default channels visible to
  @everyone (5 chattable), which conflicts with Member gating. Owner chose to
  keep gating and grant 👤 Member with a bot instead (see below).
- #announcements: staff (Team + Moderator) can post. Optional: deny Send for
  🛡️ Moderator there for Team-only posting.
- Verified with "View Server as Role": New Arrival → START HERE only, read-only;
  Member → public categories, posts in #general/#support, read-only #faq;
  Tester+Member → Testing Lab visible, #beta-downloads read-only, #testing-bugs
  writable, Staff HQ hidden; Moderator → everything incl. Staff HQ.

## 6. Messages

### #welcome

```
# NOCTURN
-# Windows optimisation, diagnostics and troubleshooting.

Welcome to the official NOCTURN community.

NOCTURN is a desktop app that keeps your PC fast and stable — it tunes Windows safely, explains what's slowing you down, diagnoses crashes and hardware problems, and helps you fix them, with every change reversible.

**Getting in**
1. Read the rules in <#rules>
2. Accept them when Discord asks
3. That's it — the community opens up

**Where things are**
› <#announcements> — releases and official news
› <#support> — help with NOCTURN
› <#bug-reports> — something broken? report it here
› <#feature-requests> — ideas, with community voting
› <#general> — everything else

-# Stay updated by following #announcements.
```

### #rules

```
# Community Rules
-# Short, clear, and enforced fairly.

**1 · Be respectful**
Treat every member with courtesy. Disagree with ideas, not people.

**2 · No harassment, hate speech or threats**
Zero tolerance. This includes slurs, targeted abuse and threats of any kind.

**3 · No spam or malicious links**
No flooding, mass mentions, unsolicited advertising, scams or links to malware.

**4 · Don't impersonate NOCTURN staff**
Official staff carry the 🌙 Nocturn Team or 🛡️ Moderator role. Staff will never ask for your password or payment details.

**5 · Use the right channel**
Support in <#support>, bugs in <#bug-reports>, ideas in <#feature-requests>.

**6 · Keep beta builds and unreleased information confidential**
Don't share pre-release builds, screenshots or details outside the Testing Lab.

**7 · Follow moderator instructions**
If you disagree with a decision, message a moderator privately.

**8 · Report problems, don't argue in public**
If someone breaks a rule, report the message or ping a moderator rather than escalating.

**Enforcement**
Warning → timeout → removal. Serious violations may result in an immediate ban.

-# By participating you also agree to Discord's Terms of Service and Community Guidelines.
```

### Announcement format (#announcements)

```
# NOCTURN 0.6.0
-# Release · 11 November 2026

One-line summary of what this release means for users.

**Highlights**
› Feature or fix, in plain language
› Feature or fix, in plain language
› Feature or fix, in plain language

**Get it**
Update from inside NOCTURN, or download from <link>.

Full details in <#changelog>.
```

Types use the same shape; only the subtitle changes:
`Release` · `Feature` · `Important fix` · `Beta` · `Maintenance`.
Maintenance posts replace **Highlights** with **When** and **What to expect**.
Ping @everyone only for releases, beta launches and maintenance that affects users.

### Changelog format (#changelog)

```
## NOCTURN 0.6.0
-# 11 November 2026

**New**
- …

**Improved**
- …

**Fixed**
- …

**Known issues**
- …
```

### #server-status (pinned)

```
## Service status
-# Updated by the NOCTURN team

**Downloads** · Operational
**Updates** · Operational
**AI assistant** · Operational

Status values: Operational · Degraded · Outage · Maintenance
-# If something is down and not listed here, post in #support.
```

### #roadmap (first post)

```
# Roadmap
-# What we're working on. Plans can change; nothing here is a release date.

**Now**
› PresentMon FPS capture for benchmarks and gaming sessions
› Driver and crash analysis refinements

**Next**
› Cross-module app profiles with automatic switching improvements
› More AI assistant tools across System pages

**Later**
› Ideas from <#feature-requests> that the community and team agree on

-# Updated with each release.
```

### #support — forum post guidelines

```
One issue per post. Please include:

**NOCTURN version** — Settings › About
**Windows version** — e.g. Windows 11 24H2 (Win + R → winver)
**PC specs** — CPU, GPU, RAM, storage
**What's wrong** — what you did, what happened, what you expected
**Screenshots / error messages** — if you have them

Tag your post, and mark it Answered once you're sorted.
For private matters (account, licence), ask in your post and a staff member will DM you.
```

### #faq

```
# FAQ

**Is NOCTURN safe to use?**
Every change NOCTURN makes is journaled and can be rolled back from the Restore centre. It never installs kernel drivers.

**Why doesn't NOCTURN show my CPU temperature?**
Windows has no vendor-neutral way to read it without loading a kernel driver, which NOCTURN won't do.

**Which GPUs are supported?**
NVIDIA: usage, VRAM, temperature, clocks and power. AMD and Intel: usage and VRAM.

**Where do I report a bug?**
<#bug-reports> for released versions. Testers use <#testing-bugs> for pre-release builds.

**How do I become a tester?**
Watch <#announcements> — we open tester applications there.

**Will my feature request be built?**
Votes help us prioritise, but they don't guarantee implementation.
```

### #known-issues (template)

```
**Issue** · Short title
**Affects** · Version(s)
**Status** · Investigating / Fix in progress / Fixed in x.y.z
**Workaround** · Steps, or "none yet"
```

### #guides (first post)

```
# Guides
-# Short how-tos. More are added over time.

› **Getting started** — first launch, choosing a profile, reading your health overview
› **Gaming modes** — Competitive, Maximum Quality, Balanced Gaming, Streaming
› **Undoing changes** — using the Restore centre
› **Reporting a crash** — finding the Crash analyser report to attach in <#support>
```

### #bug-reports — forum post guidelines

```
One bug per post. Title = short summary of the bug.

**What happened**
**What should have happened**
**Steps to reproduce**
1.
2.
3.
**NOCTURN version** — Settings › About
**PC specs** — CPU, GPU, RAM, Windows version
**Screenshots or logs**

Staff will tag your post:
`Investigating` → `Confirmed` → `Fixed` — or `Cannot Reproduce` if we need more detail.
Please search existing posts first and add to them instead of duplicating.
```

### #feature-requests — forum post guidelines

```
One idea per post. Title = the feature name.

**Problem it solves**
**Suggested solution**
**Why it would benefit users**

Vote with ⬆️ on ideas you'd use.
Votes help us prioritise — they don't guarantee a feature will be built, and the team makes the final call on scope and timing.
```

### #testing-guidelines

```
# Testing Guidelines
-# Thanks for helping make NOCTURN better.

**What testers do**
› Test pre-release builds from <#beta-downloads>
› Report reproducible bugs in <#testing-bugs>
› Share constructive feedback in <#testing-feedback>

**Good bug reports**
Clear title, steps to reproduce, expected vs actual result, build number, PC specs, and logs or screenshots. Retest before reporting and say how often it happens.

**Confidentiality**
Pre-release builds, features and screenshots are confidential until they are announced publicly.
› Never share or re-upload beta builds
› Never post unreleased details outside the Testing Lab
› Ask the team first if you're unsure

**Expect rough edges**
Beta builds can be unstable. Test on a PC you can restore, and use NOCTURN's Restore centre to undo changes.

-# Breaking confidentiality ends tester access.
```

### #tester-announcements (format)

```
## Test cycle · NOCTURN 0.6.0-beta.1
-# Runs until <date>

**Focus areas**
› …

**Download** · <#beta-downloads>
**Report** · <#testing-bugs> · <#testing-feedback>
```

### Moderation procedure (pinned in #moderation-log)

```
## Moderation procedure

**1 · Remove** — delete rule-breaking content.
**2 · Warn** — first minor offence: polite DM or in-channel note.
**3 · Timeout** — repeat or moderate offence: 1 hour, then 24 hours.
**4 · Kick** — continued disruption after timeouts.
**5 · Ban** — immediate for hate speech, threats, scams, malware, doxxing or leaking beta builds. Repeat offenders after a kick.

**Log every action here:**
`User` · `Action` · `Rule` · `Reason / evidence link` · `Moderator`

**Escalate to 🌙 Nocturn Team** for bans of long-standing members, legal or safety concerns, staff conduct, and leaks.
```
