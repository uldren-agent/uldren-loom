import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..");
const outDir = path.join(repo, "target", "verify-apps");

async function loadPlaywright() {
  try {
    return await import("playwright");
  } catch (error) {
    console.error("verify-apps: Playwright is not installed.");
    console.error("verify-apps: run `npm --prefix tools/verify-apps install`.");
    throw error;
  }
}

async function readRepoFile(...parts) {
  return await readFile(path.join(repo, ...parts), "utf8");
}

function injectJson(html, expression, value) {
  return html.replace(expression, JSON.stringify(value));
}

async function renderTemplate(app, replacements) {
  let html = await readRepoFile("crates", "loom-mcp", "src", "internal_apps", app, "index.html");
  const shell = await readRepoFile("crates", "loom-mcp", "src", "internal_apps", "app_shell.css");
  html = html.replace(/{{\s*loom\.app_shell\.css\s*}}/g, shell);
  for (const [expression, value] of replacements) {
    html = injectJson(html, expression, value);
  }
  if (html.includes("{{")) {
    throw new Error(`${app}: unresolved template expression remains`);
  }
  return html;
}

async function assertVisible(page, selector, label) {
  const locator = page.locator(selector).first();
  await locator.waitFor({ state: "visible", timeout: 3000 });
  const box = await locator.boundingBox();
  if (!box || box.width <= 0 || box.height <= 0) {
    throw new Error(`${label} has no visible layout box`);
  }
}

async function assertText(page, selector, expected, label) {
  const text = await page.locator(selector).first().innerText({ timeout: 3000 });
  if (!text.includes(expected)) {
    throw new Error(`${label} expected ${JSON.stringify(expected)} in ${JSON.stringify(text)}`);
  }
}

async function writeRenderedHtml(name, html) {
  await writeFile(path.join(outDir, `${name}.html`), html);
}

async function verifyVcs(browser) {
  const html = await renderTemplate("vcs", [
    [
      /{{\s*loom\.vcs\s*\|\s*tojson\s*}}/g,
      {
        workspace: { name: "app-fixture", head: "b3:commit-main" },
        status: {
          ok: true,
          value: {
            staged: [{ path: "README.md", kind: "modified" }],
            unstaged: [{ path: "src/lib.rs", kind: "modified" }],
            untracked: ["notes/new.md"],
            conflicts: []
          }
        },
        history: { ok: true, items: ["b3:commit-main", "b3:commit-parent"] },
        tags: { ok: true, items: ["v0.1.0"] }
      }
    ],
    [/{{\s*meta\.ui\.availableDisplayModes\s*\|\s*tojson\s*}}/g, ["inline", "fullscreen"]]
  ]);
  await writeRenderedHtml("vcs", html);
  const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "main", "VCS main");
  await assertText(page, "[data-count='staged']", "1", "VCS staged count");
  await assertText(page, "[data-table='status']", "README.md", "VCS status table");
  await page.locator("[data-tab='history']").click();
  await assertText(page, "[data-table='history']", "b3:commit-main", "VCS history table");
  await page.screenshot({ path: path.join(outDir, "vcs.png"), fullPage: true });
  await page.close();
}

async function verifyDecisions(browser) {
  const html = await renderTemplate("decisions", [
    [
      /{{\s*loom\.ask\s*\|\s*tojson\s*}}/g,
      {
        workspace: { name: "app-fixture" },
        current: {
          id: "ask-1",
          status: "pending",
          questions: [
            {
              question: "Which serving surface should be promoted first?",
              context: "The owner needs one promoted surface before expanding the app bundle.",
              examples: "A: admin first\nB: CAS first",
              shape: "radio",
              options: [
                { label: "A", description: "Promote admin first." },
                { label: "B", description: "Promote CAS first." }
              ],
              recommendation: "A keeps policy and observability first."
            }
          ]
        }
      }
    ],
    [/{{\s*meta\.ui\.availableDisplayModes\s*\|\s*tojson\s*}}/g, ["inline", "fullscreen"]]
  ]);
  await writeRenderedHtml("decisions", html);
  const page = await browser.newPage({ viewport: { width: 900, height: 720 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, ".card", "Decisions card");
  await assertText(page, ".question", "Which serving surface", "Decisions question");
  await page.locator("label.option").first().click();
  await assertVisible(page, "button.primary", "Decisions submit");
  await page.screenshot({ path: path.join(outDir, "decisions.png"), fullPage: true });
  await page.close();
}

async function verifyDirectedGraph(browser) {
  const html = await renderTemplate("directed_graph", [
    [
      /{{\s*loom\.graph\s*\|\s*tojson\s*}}/g,
      {
        workspace: { name: "app-fixture" },
        definition: { display_name: "Studio app catalog" },
        catalog: { apps: 3 },
        nodes: [
          { id: "ticket-details", label: "Ticket Details" },
          { id: "meeting-details", label: "Meeting Details" },
          { id: "decision-log", label: "Decision Log" }
        ],
        edges: [
          { from: "meeting-details", to: "ticket-details", kind: "promotes" },
          { from: "decision-log", to: "ticket-details", kind: "references" }
        ]
      }
    ]
  ]);
  await writeRenderedHtml("directed-graph", html);
  const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "svg#graph", "Directed Graph canvas");
  await assertText(page, "#node-count", "3", "Directed Graph node count");
  await assertText(page, "#edge-count", "2", "Directed Graph edge count");
  const nodes = await page.locator("svg#graph .node").count();
  const edges = await page.locator("svg#graph .edge").count();
  if (nodes !== 3 || edges !== 2) {
    throw new Error(`Directed Graph rendered ${nodes} nodes and ${edges} edges`);
  }
  await page.screenshot({ path: path.join(outDir, "directed-graph.png"), fullPage: true });
  await page.close();
}

function pagesFixture(appId, displayName) {
  return {
    workspace: { name: "app-fixture" },
    profile_id: "workspace-uuid",
    definition: {
      app_id: appId,
      display_name: displayName,
      resource_uri: `ui://app-fixture/mcp/apps/${appId}`,
      projection_refs: ["view:pages.document", "view:pages.structure"],
      read_tools: ["pages_get", "structures_get", "substrate_refs"],
      write_tools: ["pages_update", "pages_publish", "structures_link_node"],
      subscription_refs: ["changes:pages", "changes:refs"]
    },
    spaces: [
      { space_id: "specs", title: "Specs", archived: false },
      { space_id: "plans", title: "Plans", archived: false }
    ],
    page: {
      page_id: "spec-001",
      title: "Spec 001",
      status: "draft",
      current_revision: 3,
      rendered_body: "Published introduction\n\nA stable document body.",
      draft_rendered_body: "Draft introduction\n\nUpdated text ready for review."
    },
    history: [
      { revision: 1, kind: "revision" },
      { revision: 2, kind: "revision" },
      { revision: 3, kind: "revision" }
    ],
    structure: {
      structure: { title: "Launch Plan", kind: "mindmap" },
      nodes: [
        { node_id: "root", label: "Launch", kind: "topic", entity_ref: "page:spec-001" },
        { node_id: "design", label: "Design", kind: "topic", entity_ref: "page:design" },
        { node_id: "build", label: "Build", kind: "topic", entity_ref: "ticket:CORE-12" }
      ],
      edges: [
        { edge_id: "e1", src_node_id: "root", dst_node_id: "design", label: "contains" },
        { edge_id: "e2", src_node_id: "root", dst_node_id: "build", label: "contains" }
      ]
    }
  };
}

function ticketsFixture(appId, displayName) {
  const tickets = [
    {
      workspace_id: "workspace-uuid",
      ticket_id: "ticket-a",
      project_id: "CORE",
      primary_key: "CORE-1",
      ticket_type: "story",
      external_source: "fixture",
      external_id: "A",
      fields: {
        summary: "Build ticket board app",
        status: "in_progress",
        status_category: "in_progress",
        assignee: "principal:alice",
        sprint: "Sprint 1",
        story_points: 5,
        due_date: "2026-08-01"
      },
      policy_labels: [],
      profile_root: "b3:ticket-a",
      operation_id: "op-a",
      sequence: 1
    },
    {
      workspace_id: "workspace-uuid",
      ticket_id: "ticket-b",
      project_id: "CORE",
      primary_key: "CORE-2",
      ticket_type: "bug",
      external_source: "fixture",
      external_id: "B",
      fields: {
        summary: "Triage stale workflow actions",
        status: "todo",
        status_category: "todo",
        sprint: "Backlog",
        story_points: 2
      },
      policy_labels: [],
      profile_root: "b3:ticket-b",
      operation_id: "op-b",
      sequence: 2
    }
  ];
  return {
    app_uri: `ui://app-fixture/mcp/apps/${appId}`,
    workspace: { name: "app-fixture" },
    profile_id: "workspace-uuid",
    definition: {
      app_id: appId,
      display_name: displayName,
      resource_uri: `ui://app-fixture/mcp/apps/${appId}`,
      projection_refs: ["view:tickets.detail", "view:tickets.board"],
      read_tools: ["tickets_get", "tickets_history"],
      write_tools: ["tickets_update"],
      subscription_refs: ["changes:tickets"]
    },
    tickets,
    ticket: appId === "ticket-details" ? tickets[0] : null,
    history: [
      { sequence: 1, operation_kind: "create", operation_id: "op-a" },
      { sequence: 2, operation_kind: "update", operation_id: "op-b" }
    ],
    refs: { inbound: [{ source_id: "page:plan" }], outbound: [] },
    lanes: [
      {
        lane_id: "lane-1",
        lane_key: "current",
        owner_principal: "principal:alice",
        lane_status: "working",
        lane_tickets: [{ ticket_id: "ticket-a", rank: 10 }],
        active_ticket_id: "ticket-a",
        status_report: "working",
        reviewer_feedback: "",
        updated_at: 1,
        updated_by: "principal:alice"
      }
    ]
  };
}

async function verifyTicketsApp(browser, appId, displayName, expected) {
  const html = await renderTemplate("ticket_planning", [
    [/{{\s*loom\.tickets\s*\|\s*tojson\s*}}/g, ticketsFixture(appId, displayName)]
  ]);
  await writeRenderedHtml(appId, html);
  const page = await browser.newPage({ viewport: { width: 1180, height: 780 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "main", `${displayName} main`);
  await assertText(page, "#title", displayName, `${displayName} title`);
  await assertText(page, "#metrics", "Tickets", `${displayName} metrics`);
  await assertText(page, "#columns", expected, `${displayName} grouping`);
  await assertText(page, "#selected", "CORE-1", `${displayName} selected ticket`);
  await assertText(page, "#lanes", "current", `${displayName} lanes`);
  await page.screenshot({ path: path.join(outDir, `${appId}.png`), fullPage: true });
  await page.close();
}

function chatFixture(appId, displayName) {
  return {
    app_uri: `ui://app-fixture/mcp/apps/${appId}`,
    workspace: { name: "app-fixture" },
    profile_id: "workspace-uuid",
    definition: {
      app_id: appId,
      display_name: displayName,
      resource_uri: `ui://app-fixture/mcp/apps/${appId}`,
      projection_refs: ["view:chat.channel"],
      read_tools: ["chat_channels", "chat_messages", "chat_fetch_events"],
      write_tools: ["chat_post_message", "chat_update_cursor"],
      subscription_refs: ["changes:chat"]
    },
    channels: [
      {
        workspace_id: "workspace-uuid",
        channel_id: "channel-general",
        channel_handle: "general",
        channel_name: "General"
      }
    ],
    channel: {
      workspace_id: "workspace-uuid",
      channel_id: "channel-general",
      channel_handle: "general",
      channel_name: "General",
      messages: [
        {
          message_id: "m1",
          thread_id: null,
          body: Array.from(new TextEncoder().encode("Welcome to the launch room")),
          author_principal: "principal:alice",
          created_at_ms: 1,
          updated_at_ms: 1,
          redacted: false,
          reactions: [{ kind: "approved", principal: "principal:bob" }]
        },
        {
          message_id: "m2",
          thread_id: "t1",
          body: Array.from(new TextEncoder().encode("Thread reply with the latest status")),
          author_principal: "principal:bob",
          created_at_ms: 2,
          updated_at_ms: 2,
          redacted: false,
          reactions: []
        }
      ],
      threads: [{ thread_id: "t1", parent_message_id: "m1", created_at_ms: 2 }],
      tasks: [
        {
          task_id: "task-1",
          message_id: "m1",
          title: "Follow up with release owner",
          created_by: "principal:alice",
          created_at_ms: 3,
          state: { kind: "Claimed", claim_id: "claim-1", claimant_principal: "principal:bob" }
        }
      ],
      agent_invocations: [
        {
          invocation_id: "invoke-1",
          agent_principal: "principal:agent",
          requested_by: "principal:alice",
          requested_at_ms: 4,
          source_message_ids: ["m1"],
          prompt: Array.from(new TextEncoder().encode("Summarize blockers")),
          reply_message_ids: ["m2"]
        }
      ],
      handoffs: [
        {
          handoff_id: "handoff-1",
          from_agent_principal: "principal:agent",
          to_principal: "principal:bob",
          requested_by: "principal:alice",
          requested_at_ms: 5,
          reason: "Needs human owner"
        }
      ]
    },
    selected_thread: appId === "chat-thread"
      ? { thread_id: "t1", parent_message_id: "m1", created_at_ms: 2 }
      : null,
    cursor: {
      workspace_id: "workspace-uuid",
      channel_id: "channel-general",
      channel_handle: "general",
      principal: "principal:alice",
      next_sequence: 1,
      head_sequence: 6,
      unread_count: 5
    },
    presence: [
      {
        workspace_id: "workspace-uuid",
        channel_id: "channel-general",
        principal: "principal:alice",
        status: "active",
        expires_at_ms: 30000
      }
    ],
    events: { events: [{ sequence: 1, operation_kind: "chat.message.created" }], next: "cursor" },
    emoji: { workspace_id: "workspace-uuid", custom: ["approved"] }
  };
}

function driveFixture(appId, displayName) {
  return {
    app_uri: `ui://app-fixture/mcp/apps/${appId}`,
    workspace: { name: "app-fixture" },
    profile_id: "workspace-uuid",
    definition: {
      app_id: appId,
      display_name: displayName,
      resource_uri: `ui://app-fixture/mcp/apps/${appId}`,
      projection_refs: ["view:drive.folder", "view:drive.file"],
      read_tools: ["drive_list", "drive_read", "drive_list_versions"],
      write_tools: ["drive_create_upload", "drive_commit_upload"],
      subscription_refs: ["changes:drive"]
    },
    folder: {
      workspace_id: "workspace-uuid",
      folder_id: "root",
      profile_root: "b3:drive-root",
      entries: [
        { name: "Specs", fold_key: "specs", node_id: "folder-1", kind: "folder" },
        { name: "Plan.txt", fold_key: "plan.txt", node_id: "file-1", kind: "file" },
        {
          name: "Plan (conflicted copy 2026-07-16 12-00-00).txt",
          fold_key: "plan conflicted",
          node_id: "file-2",
          kind: "file"
        }
      ]
    },
    stat: null,
    selected_file: appId === "drive-preview" ? { file_id: "file-1" } : null,
    file_bytes: appId === "drive-preview"
      ? Array.from(new TextEncoder().encode("Drive preview bytes"))
      : null,
    versions: [
      {
        file_id: "file-1",
        version: 1,
        operation_id: "op-1",
        author_principal: "principal:alice",
        timestamp_ms: 100,
        content_digest: "b3:file-content",
        manifest_digest: null,
        size: 19
      }
    ],
    conflicts: [
      {
        conflict_id: "upload-2:conflict",
        folder_id: "root",
        visible_node_id: "file-1",
        conflict_node_id: "file-2",
        conflict_name: "Plan (conflicted copy 2026-07-16 12-00-00).txt",
        base_root: "b3:old-root",
        resolution: "open"
      }
    ],
    shares: [
      {
        grant_id: "grant-1",
        target_kind: "file",
        target_id: "file-1",
        principal: "principal:bob",
        role: "viewer",
        granted_by: "principal:alice",
        granted_at_ms: 100,
        expires_at_ms: 500
      }
    ],
    retention: [
      {
        pin_id: "pin-1",
        kind: "current_root",
        root: "b3:drive-root",
        target_entity_id: "drive:file:file-1",
        added_by: "principal:alice",
        added_at_ms: 100,
        expires_at_ms: null
      }
    ],
    lease_tools: [
      {
        tool: "drive_acquire_lease",
        target: "file or folder",
        description: "Acquire an attached-daemon write-intent lease"
      }
    ]
  };
}

function meetingsFixture(appId, displayName) {
  const annotations = [
    {
      annotation_id: "decision/note-1/0",
      meeting_id: "meeting/note-1",
      source_span_ids: ["span/note-1/0"],
      kind: "Decision",
      label: "Keep source payloads.",
      normalized_id: null,
      confidence_ppm: null,
      evidence_digest: null,
      extractor: null,
      status: "observed",
      created_at_ms: 701,
      accepted_by: null,
      accepted_at_ms: null
    },
    {
      annotation_id: "task/note-1/0",
      meeting_id: "meeting/note-1",
      source_span_ids: ["span/note-1/0"],
      kind: "Task",
      label: "Create the follow-up ticket.",
      normalized_id: null,
      confidence_ppm: null,
      evidence_digest: null,
      extractor: null,
      status: "suggested",
      created_at_ms: 702,
      accepted_by: null,
      accepted_at_ms: null
    }
  ];
  return {
    app_uri: `ui://app-fixture/mcp/apps/${appId}`,
    workspace: { name: "app-fixture" },
    profile_id: "workspace-uuid",
    definition: {
      app_id: appId,
      display_name: displayName,
      resource_uri: `ui://app-fixture/mcp/apps/${appId}`,
      projection_refs: ["view:meetings.detail", "view:meetings.extraction-review"],
      read_tools: ["meetings_list", "meetings_get", "meetings_projection_outputs"],
      write_tools: appId === "meeting-search" || appId === "access-audit"
        ? []
        : ["meetings_accept_annotation", "meetings_reject_annotation", "meetings_import_snapshot"],
      subscription_refs: ["changes:meetings"]
    },
    list: {
      workspace_id: "workspace-uuid",
      total: 1,
      offset: 0,
      limit: 50,
      meetings: [
        {
          meeting_id: "meeting/note-1",
          title: "Architecture review",
          starts_at_ms: 700,
          ends_at_ms: 760,
          status: "active",
          source_refs: ["source/note-1"],
          updated_at_ms: 760
        }
      ]
    },
    meeting: {
      workspace_id: "workspace-uuid",
      meeting_id: "meeting/note-1",
      title: "Architecture review",
      starts_at_ms: 700,
      ends_at_ms: 760,
      calendar_event_ref: "calendar:event-1",
      owner_principal: "principal:alice",
      attendee_refs: ["principal:alice", "principal:bob"],
      folder_refs: ["folder:meetings"],
      source_refs: ["source/note-1"],
      current_source_digest: "b3:meeting-source",
      summary_ref: "source/note-1/summary.txt",
      status: "active",
      created_at_ms: 700,
      updated_at_ms: 760,
      annotations
    },
    projection: {
      workspace_id: "workspace-uuid",
      profile_root: "b3:meetings-root",
      outputs: [
        {
          output_id: "out-document-note-1",
          projection: "document",
          action: "upsert",
          output_ref: "document:meeting/note-1",
          entity_kind: "meeting",
          entity_id: "meeting/note-1",
          source_ids: ["source/note-1"],
          payload_cbor_hex: "a0",
          redaction_state: "live",
          recorded_at_ms: 761,
          record_cbor_hex: "a1"
        }
      ],
      output_set_cbor_hex: "a2"
    },
    review: {
      workspace_id: "workspace-uuid",
      suggested_annotation_ids: ["task/note-1/0"],
      accepted_annotation_ids: [],
      rejected_annotation_ids: [],
      vocabulary_terms: 2,
      review_cbor_hex: "a3"
    },
    import_coverage: {
      status: "source-backed list and import tool",
      import_tool: "meetings_import_snapshot",
      imported_rows: 1,
      gaps: []
    },
    access_audit: {
      status: "target",
      read_policy: "shared Studio ACL",
      reason: "Meetings-specific audit projection is not promoted yet"
    }
  };
}

async function verifyChatApp(browser, appId, displayName, expected) {
  const html = await renderTemplate("chat", [
    [/{{\s*loom\.chat\s*\|\s*tojson\s*}}/g, chatFixture(appId, displayName)]
  ]);
  await writeRenderedHtml(appId, html);
  const page = await browser.newPage({ viewport: { width: 1180, height: 780 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "main", `${displayName} main`);
  await assertText(page, "#title", displayName, `${displayName} title`);
  await assertText(page, "#channels", "general", `${displayName} channel list`);
  await assertText(page, "#messages", expected, `${displayName} message panel`);
  await assertText(page, "#tasks", "Follow up", `${displayName} tasks`);
  await assertText(page, "#presence-list", "active", `${displayName} presence`);
  await assertText(page, "#handoffs", "handoff-1", `${displayName} handoffs`);
  await page.screenshot({ path: path.join(outDir, `${appId}.png`), fullPage: true });
  await page.close();
}

async function verifyDriveApp(browser, appId, displayName, expected) {
  const html = await renderTemplate("drive", [
    [/{{\s*loom\.drive\s*\|\s*tojson\s*}}/g, driveFixture(appId, displayName)]
  ]);
  await writeRenderedHtml(appId, html);
  const page = await browser.newPage({ viewport: { width: 1180, height: 780 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "main", `${displayName} main`);
  await assertText(page, "#title", displayName, `${displayName} title`);
  await assertText(page, "#entries", "Plan.txt", `${displayName} entries`);
  await assertText(page, "#conflicts", "upload-2:conflict", `${displayName} conflicts`);
  await assertText(page, "#shares", "grant-1", `${displayName} shares`);
  await assertText(page, "#retention", "pin-1", `${displayName} retention`);
  await assertText(page, "#preview", expected, `${displayName} preview`);
  await page.screenshot({ path: path.join(outDir, `${appId}.png`), fullPage: true });
  await page.close();
}

async function verifyMeetingsApp(browser, appId, displayName, expected) {
  const html = await renderTemplate("meetings", [
    [/{{\s*loom\.meetings\s*\|\s*tojson\s*}}/g, meetingsFixture(appId, displayName)]
  ]);
  await writeRenderedHtml(appId, html);
  const page = await browser.newPage({ viewport: { width: 1180, height: 780 } });
  await page.setContent(html, { waitUntil: "load" });
  await assertVisible(page, "main", `${displayName} main`);
  await assertText(page, "#title", displayName, `${displayName} title`);
  await assertText(page, "#meetings", "Architecture review", `${displayName} meeting list`);
  await assertText(page, "#annotations", "Keep source payloads", `${displayName} annotations`);
  await assertText(page, "#outputs", "out-document-note-1", `${displayName} outputs`);
  await assertText(page, expected.selector, expected.text, expected.label);
  await page.screenshot({ path: path.join(outDir, `${appId}.png`), fullPage: true });
  await page.close();
}

async function verifyPagesApp(browser, directory, appId, displayName, checks) {
  const html = await renderTemplate(directory, [
    [/{{\s*loom\.pages\s*\|\s*tojson\s*}}/g, pagesFixture(appId, displayName)]
  ]);
  await writeRenderedHtml(appId, html);
  const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
  await page.setContent(html, { waitUntil: "load" });
  for (const check of checks) {
    if (check.kind === "visible") {
      await assertVisible(page, check.selector, check.label);
    } else {
      await assertText(page, check.selector, check.expected, check.label);
    }
  }
  await page.screenshot({ path: path.join(outDir, `${appId}.png`), fullPage: true });
  await page.close();
}

async function main() {
  const { chromium } = await loadPlaywright();
  await mkdir(outDir, { recursive: true });
  let browser;
  try {
    browser = await chromium.launch();
  } catch (error) {
    console.error("verify-apps: Chromium could not be launched for Playwright.");
    console.error("verify-apps: if the browser is missing, run `npm --prefix tools/verify-apps exec playwright install chromium`.");
    throw error;
  }
  try {
    await verifyVcs(browser);
    await verifyDecisions(browser);
    await verifyDirectedGraph(browser);
    await verifyTicketsApp(browser, "ticket-details", "Ticket Details", "CORE-1");
    await verifyTicketsApp(browser, "board", "Board", "in_progress");
    await verifyTicketsApp(browser, "roadmap", "Roadmap", "2026-08-01");
    await verifyTicketsApp(browser, "sprint-planner", "Sprint Planner", "Sprint 1");
    await verifyTicketsApp(browser, "backlog-triage", "Backlog Triage", "needs triage");
    await verifyTicketsApp(browser, "dashboards", "Dashboards", "CORE");
    await verifyChatApp(browser, "chat-channel", "Chat Channel", "Welcome to the launch room");
    await verifyChatApp(browser, "chat-thread", "Chat Thread", "Thread reply");
    await verifyChatApp(browser, "chat-tasks", "Chat Tasks", "Welcome to the launch room");
    await verifyChatApp(browser, "chat-presence", "Chat Presence", "Welcome to the launch room");
    await verifyChatApp(browser, "chat-handoffs", "Chat Handoffs", "Welcome to the launch room");
    await verifyDriveApp(browser, "drive-browser", "Drive Browser", "Select a file-backed");
    await verifyDriveApp(browser, "drive-preview", "Drive Preview", "Drive preview bytes");
    await verifyDriveApp(browser, "drive-sharing", "Drive Sharing", "Select a file-backed");
    await verifyDriveApp(browser, "drive-conflicts", "Drive Conflicts", "Select a file-backed");
    await verifyDriveApp(browser, "drive-retention", "Drive Retention", "Select a file-backed");
    await verifyMeetingsApp(browser, "meeting-details", "Meeting Details", {
      selector: "#details",
      text: "meeting/note-1",
      label: "Meeting Details body"
    });
    await verifyMeetingsApp(browser, "memory-graph", "Memory Graph", {
      selector: "#graph",
      text: "Create the follow-up ticket",
      label: "Memory Graph nodes"
    });
    await verifyMeetingsApp(browser, "extraction-review", "Extraction Review", {
      selector: "#review",
      text: "Suggested",
      label: "Extraction Review status"
    });
    await verifyMeetingsApp(browser, "meeting-search", "Meeting Search", {
      selector: "#tools",
      text: "No write tools",
      label: "Meeting Search read-only tools"
    });
    await verifyMeetingsApp(browser, "import-coverage", "Import Coverage", {
      selector: "#coverage",
      text: "meetings_import_snapshot",
      label: "Import Coverage tool"
    });
    await verifyMeetingsApp(browser, "access-audit", "Access Audit", {
      selector: "#audit",
      text: "shared Studio ACL",
      label: "Access Audit policy"
    });
    await verifyPagesApp(browser, "document_viewer", "document-viewer", "Spec Document Viewer", [
      { kind: "visible", selector: "article.document", label: "Document Viewer body" },
      { selector: "#title", expected: "Spec 001", label: "Document Viewer title" },
      { selector: "#spaces", expected: "Specs", label: "Document Viewer spaces" }
    ]);
    await verifyPagesApp(browser, "mind_map", "mind-map", "Mind Map", [
      { kind: "visible", selector: "svg#map", label: "Mind Map canvas" },
      { selector: "#summary", expected: "Launch Plan", label: "Mind Map summary" }
    ]);
    await verifyPagesApp(browser, "canvas", "canvas", "Canvas", [
      { kind: "visible", selector: ".board", label: "Canvas board" },
      { selector: "#summary", expected: "Launch Plan", label: "Canvas summary" }
    ]);
    await verifyPagesApp(browser, "diagram_editor", "diagram-editor", "Diagram Editor", [
      { kind: "visible", selector: "svg#diagram", label: "Diagram canvas" },
      { selector: "#bindings", expected: "page:spec-001", label: "Diagram bindings" }
    ]);
  } finally {
    await browser.close();
  }
  console.log(`verify-apps: wrote rendered app fixtures and screenshots to ${outDir}`);
}

await main();
