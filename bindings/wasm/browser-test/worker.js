import init, { LoomSql, blob_digest, runtime_profile, version } from '../pkg/loom_wasm.js';

const bytes = (value) => new TextEncoder().encode(value);
const text = (value) => new TextDecoder().decode(value);

const assert = (condition, label) => {
  if (!condition) {
    throw new Error(label);
  }
};

const assertEquals = (expected, actual, label) => {
  if (expected !== actual) {
    throw new Error(`${label}: expected ${expected}, got ${actual}`);
  }
};

const assertBytes = (expected, actual, label) => {
  assert(actual instanceof Uint8Array, `${label}: expected Uint8Array`);
  assertEquals(text(expected), text(actual), label);
};

const run = async () => {
  await init();
  assert(version().length > 0, 'version');
  assert(blob_digest(bytes('abc')).startsWith('blake3:'), 'blob digest');
  assert(runtime_profile().length > 0, 'runtime profile');

  const path = `runtime-${crypto.randomUUID()}.loom`;
  const db = await LoomSql.create(path, 'app', 'main', 'default', undefined, undefined);
  try {
    const nsId = db.workspace_create('work', 'files');
    let listed = db.workspace_list_json();
    assert(listed.includes(nsId), 'workspace id');
    assert(listed.includes('"work"'), 'workspace name');
    assert(listed.includes('"files"'), 'workspace facet');
    db.workspace_rename('work', 'working');
    listed = db.workspace_list_json();
    assert(listed.includes('"working"'), 'workspace rename');
    db.workspace_delete(nsId);
    listed = db.workspace_list_json();
    assert(!listed.includes('"working"'), 'workspace delete');

    const bootstrap = JSON.parse(db.identity_list_json());
    assertEquals(false, bootstrap.authenticated_mode, 'bootstrap auth mode');
    const root = bootstrap.root;
    db.identity_set_passphrase(root, 'root-pass');
    try {
      db.identity_list_json();
      throw new Error('identity list before auth: expected failure');
    } catch (error) {
      assert(String(error).includes('AUTHENTICATION_FAILED'), 'identity list before auth');
    }
    db.authenticate_passphrase(root, 'root-pass');
    const alice = db.identity_add_principal('alice', 'Alice', 'user');
    db.identity_set_passphrase(alice, 'alice-pass');
    const identity = JSON.parse(db.identity_list_json());
    assertEquals(true, identity.authenticated_mode, 'authenticated mode');
    assert(identity.principals.some((principal) => principal.id === alice), 'new principal');
    const reader = identity.roles.find((role) => role.name === 'reader').id;
    db.identity_assign_role(alice, reader);
    assert(
      JSON.parse(db.identity_list_json()).principals.some((principal) =>
        principal.id === alice && principal.roles.includes(reader)
      ),
      'assigned reader role'
    );
    assertEquals(true, db.identity_revoke_role(alice, reader), 'role revoke');
    assertEquals(false, db.identity_revoke_role(alice, reader), 'role revoke absent');
    db.acl_grant(0, alice, undefined, 'files', 1);
    const grants = JSON.parse(db.acl_list_json());
    assert(grants.some((grant) =>
      grant.subject === alice && grant.domain === 'files' && grant.rights.includes('read')
    ), 'acl grant');
    assertEquals(true, db.acl_revoke(0, alice, undefined, 'files', 1), 'acl revoke');
    assertEquals(false, db.acl_revoke(0, alice, undefined, 'files', 1), 'acl revoke absent');
    db.acl_grant(0, alice, undefined, 'files', 1, "principal == 'alice'");
    const predicateGrants = JSON.parse(db.acl_list_json());
    assert(predicateGrants.some((grant) =>
      grant.subject === alice &&
      grant.predicate?.language === 'cel' &&
      grant.predicate?.expression === "principal == 'alice'"
    ), 'acl predicate grant');
    assertEquals(
      true,
      db.acl_revoke(0, alice, undefined, 'files', 1, "principal == 'alice'"),
      'acl predicate revoke'
    );

    const digest = db.cas_put('blobs', bytes('hello'));
    assertEquals(digest, db.cas_put('blobs', bytes('hello')), 'cas idempotent put');
    assert(db.cas_has('blobs', digest), 'cas has');
    assertBytes(bytes('hello'), db.cas_get('blobs', digest), 'cas get');
    assert(db.cas_list('blobs').includes(digest), 'cas list');
    assert(db.cas_get('blobs', blob_digest(bytes('missing'))) === undefined, 'cas missing');

    assertEquals('0', db.queue_append('events', 'orders', bytes('one')).toString(), 'queue first seq');
    assertEquals('1', db.queue_append('events', 'orders', bytes('two')).toString(), 'queue second seq');
    assertEquals('2', db.queue_len('events', 'orders').toString(), 'queue len');
    assertBytes(bytes('one'), db.queue_get('events', 'orders', 0n), 'queue get');
    assert(db.queue_get('events', 'orders', 9n) === undefined, 'queue missing');
    assertEquals(2, db.queue_range('events', 'orders', 0n, 2n).length, 'queue range');
    assertEquals('0', db.queue_consumer_position('events', 'orders', 'worker').toString(), 'consumer initial');
    assertEquals(2, db.queue_consumer_read('events', 'orders', 'worker', 2).length, 'consumer read');
    db.queue_consumer_advance('events', 'orders', 'worker', 2n);
    assertEquals('2', db.queue_consumer_position('events', 'orders', 'worker').toString(), 'consumer advance');
    db.queue_consumer_reset('events', 'orders', 'worker', 1n);
    assertEquals('1', db.queue_consumer_position('events', 'orders', 'worker').toString(), 'consumer reset');

    const textPut = db.doc_put_text('docs', 'notes', 'a', 'hello text');
    const textDigest = textPut.digest;
    assert(textDigest.startsWith('blake3:'), 'doc text digest');
    const textDoc = db.doc_get_text('docs', 'notes', 'a');
    assertEquals('hello text', textDoc.text, 'doc text get');
    assertEquals(textDigest, textDoc.digest, 'doc text digest get');
    assert(db.doc_get_text('docs', 'notes', 'missing') === null, 'doc text missing');
    try {
      db.doc_put_text('docs', 'notes', 'a', 'stale', blob_digest(bytes('stale')));
      throw new Error('doc stale put: expected failure');
    } catch (error) {
      assert(!String(error).includes('expected failure'), 'doc stale put');
    }
    const updatedDigest = db.doc_put_text('docs', 'notes', 'a', 'updated text', textPut.entity_tag).digest;
    assert(updatedDigest !== textDigest, 'doc guarded update');
    const binaryDigest = db.doc_put_binary('docs', 'notes', 'raw', Uint8Array.from([0xff, 0x00])).digest;
    assert(binaryDigest.startsWith('blake3:'), 'doc binary digest');
    const binaryDoc = db.doc_get_binary('docs', 'notes', 'raw');
    assertEquals(0xff, binaryDoc.bytes[0], 'doc binary first byte');
    assertEquals(0x00, binaryDoc.bytes[1], 'doc binary second byte');
    assertEquals(binaryDigest, binaryDoc.digest, 'doc binary digest get');
    assert(db.doc_list_binary('docs', 'notes').length > 0, 'doc list binary');
    try {
      db.doc_get_text('docs', 'notes', 'raw');
      throw new Error('doc non-text get: expected failure');
    } catch (error) {
      assert(String(error).includes('DOCUMENT_NOT_TEXT'), 'doc non-text get');
    }

    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
    db.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')");
    const rows = db.query('SELECT id, v FROM t ORDER BY id');
    assertEquals(2, rows.length, 'sql row count');
    assertEquals('1', rows[0][0].toString(), 'sql first id');
    assertEquals('a', rows[0][1], 'sql first value');
    assertEquals('2', rows[1][0].toString(), 'sql second id');
    assertEquals('b', rows[1][1], 'sql second value');
    assert(db.commit('seed', 'wasm').startsWith('blake3:'), 'sql commit');
  } finally {
    db.free();
  }
};

run()
  .then(() => postMessage({ ok: true }))
  .catch((error) => postMessage({ ok: false, error: error?.stack || String(error) }));
