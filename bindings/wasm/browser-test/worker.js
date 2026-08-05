import init, {
  LaneTicketPlacement,
  LoomStore,
  LoomSql,
  blob_digest,
  capabilities,
  runtime_profile,
  version,
} from '../pkg/loom_wasm.js';

const bytes = (value) => new TextEncoder().encode(value);
const text = (value) => new TextDecoder().decode(value);
const utf8 = new TextEncoder();
const utf8Decode = new TextDecoder();

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

const assertRawBytes = (expected, actual, label) => {
  assert(actual instanceof Uint8Array, `${label}: expected Uint8Array`);
  assertEquals(expected.length, actual.length, `${label} length`);
  for (let i = 0; i < expected.length; i += 1) {
    assertEquals(expected[i], actual[i], `${label} byte ${i}`);
  }
};

const concat = (chunks) => {
  const length = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const out = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.length;
  }
  return out;
};

const cborHeader = (major, length) => {
  if (length < 24) {
    return Uint8Array.of((major << 5) | length);
  }
  if (length < 256) {
    return Uint8Array.of((major << 5) | 24, length);
  }
  if (length < 65536) {
    return Uint8Array.of((major << 5) | 25, length >> 8, length & 0xff);
  }
  return Uint8Array.of(
    (major << 5) | 26,
    (length >>> 24) & 0xff,
    (length >>> 16) & 0xff,
    (length >>> 8) & 0xff,
    length & 0xff
  );
};

const cborText = (value) => {
  const encoded = utf8.encode(value);
  return concat([cborHeader(3, encoded.length), encoded]);
};

const cborUint = (value) => cborHeader(0, value);
const cborNull = () => Uint8Array.of(0xf6);
const cborArray = (items) => concat([cborHeader(4, items.length), ...items]);
const cborBytes = (value) => concat([cborHeader(2, value.length), value]);
const cborMap = (pairs) => concat([
  cborHeader(5, pairs.length),
  ...pairs.flatMap(([key, value]) => [cborText(key), value]),
]);

const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');

const uuidBytes = (value) => {
  const hexValue = value.replaceAll('-', '');
  const out = new Uint8Array(16);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(hexValue.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
};

const uvarint = (value) => {
  const out = [];
  let remaining = value;
  while (true) {
    let byte = remaining & 0x7f;
    remaining = Math.floor(remaining / 128);
    if (remaining === 0) {
      out.push(byte);
      break;
    }
    out.push(byte | 0x80);
  }
  return Uint8Array.from(out);
};

const lp = (value) => concat([uvarint(value.length), value]);

const laneCbor = () => cborArray([
  cborText('agent-order'),
  cborText('agent-order'),
  cborText('Agent order'),
  cborText('Browser placement regression'),
  cborText('assignment'),
  cborNull(),
  cborText('ready'),
  cborArray([]),
  cborNull(),
  cborText(''),
  cborText(''),
  cborUint(1),
  cborText('agent-1'),
]);

const cborDecode = (bytesValue, offset = 0) => {
  const first = bytesValue[offset++];
  const major = first >> 5;
  let info = first & 0x1f;
  let length = info;
  if (info === 24) {
    length = bytesValue[offset++];
  } else if (info === 25) {
    length = (bytesValue[offset] << 8) | bytesValue[offset + 1];
    offset += 2;
  } else if (info === 26) {
    length = new DataView(bytesValue.buffer, bytesValue.byteOffset + offset, 4).getUint32(0);
    offset += 4;
  } else if (info === 27) {
    length = Number(new DataView(bytesValue.buffer, bytesValue.byteOffset + offset, 8).getBigUint64(0));
    offset += 8;
  } else if (info >= 28) {
    throw new Error(`unsupported cbor additional info ${info}`);
  }
  if (major === 0) {
    return [length, offset];
  }
  if (major === 3) {
    const value = utf8Decode.decode(bytesValue.slice(offset, offset + length));
    return [value, offset + length];
  }
  if (major === 2) {
    return [bytesValue.slice(offset, offset + length), offset + length];
  }
  if (major === 4) {
    const items = [];
    for (let i = 0; i < length; i += 1) {
      const [item, nextOffset] = cborDecode(bytesValue, offset);
      items.push(item);
      offset = nextOffset;
    }
    return [items, offset];
  }
  if (major === 5) {
    const map = {};
    for (let i = 0; i < length; i += 1) {
      const [key, valueOffset] = cborDecode(bytesValue, offset);
      const [value, nextOffset] = cborDecode(bytesValue, valueOffset);
      map[key] = value;
      offset = nextOffset;
    }
    return [map, offset];
  }
  if (major === 7 && info === 22) {
    return [null, offset];
  }
  throw new Error(`unsupported cbor major ${major}`);
};

const laneTicketIds = (laneBytes) => cborDecode(laneBytes)[0][7].map((ticket) => ticket[0]);

const identityAuthorityState = (snapshot) => {
  let offset = 4;
  if (text(snapshot.slice(0, 4)) !== 'LID9') {
    throw new Error('identity snapshot magic');
  }
  const rootTag = snapshot[offset++];
  if (rootTag === 1) {
    offset += 16;
  } else if (rootTag !== 0) {
    throw new Error('identity snapshot root tag');
  }
  const mode = snapshot[offset++];
  const authority = snapshot.slice(offset, offset + 16);
  offset += 16;
  let generation = 0;
  let shift = 0;
  while (true) {
    const byte = snapshot[offset++];
    generation += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) {
      break;
    }
    shift += 7;
  }
  return { mode, authority, generation };
};

const authorityHandoffPayload = (from, to, generation, head) => cborArray([
  cborText('loom.identity.authority_handoff.payload.v1'),
  cborBytes(from),
  cborBytes(to),
  cborUint(generation),
  head === undefined ? cborNull() : cborBytes(head),
]);

const authorityHandoffRecord = (from, to, generation, head, keyId, signature) => {
  const payload = authorityHandoffPayload(from, to, generation, head);
  return cborArray([
    cborText('loom.identity.authority_handoff.v1'),
    cborMap([
      ['alg', cborText('ES256')],
      ['kid', cborBytes(keyId)],
    ]),
    cborBytes(payload),
    cborBytes(signature),
  ]);
};

const signedFastForwardIdentitySnapshot = async (baseSnapshot, root, keyId, privateKey) => {
  const rootBytes = uuidBytes(root);
  const keyIdBytes = uuidBytes(keyId);
  const state = identityAuthorityState(baseSnapshot);
  assertRawBytes(rootBytes, state.authority, 'authority handoff base authority');
  assertEquals(0, state.generation, 'authority handoff base generation');
  const payload = authorityHandoffPayload(rootBytes, rootBytes, 1, undefined);
  const signature = new Uint8Array(
    await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, privateKey, payload)
  );
  assertEquals(64, signature.length, 'authority handoff signature length');
  const record = authorityHandoffRecord(rootBytes, rootBytes, 1, undefined, keyIdBytes, signature);
  const authorityStart = 21;
  let offset = authorityStart + 17;
  while ((baseSnapshot[offset++] & 0x80) !== 0) {}
  offset += 1;
  while ((baseSnapshot[offset++] & 0x80) !== 0) {}
  return concat([
    baseSnapshot.slice(0, authorityStart),
    Uint8Array.of(0),
    rootBytes,
    uvarint(1),
    Uint8Array.of(0),
    uvarint(1),
    rootBytes,
    rootBytes,
    uvarint(1),
    Uint8Array.of(0),
    lp(record),
    baseSnapshot.slice(offset),
  ]);
};

const run = async () => {
  await init();
  assert(version().length > 0, 'version');
  assert(blob_digest(bytes('abc')).startsWith('blake3:'), 'blob digest');
  assert(runtime_profile().length > 0, 'runtime profile');

  const securityPath = `security-${crypto.randomUUID()}.loom`;
  const security = await LoomStore.create(securityPath, 'default', undefined, undefined);
  const securityCapabilities = cborDecode(capabilities())[0];
  const selfSignedCapability = securityCapabilities.records.find(
    (record) => record.capability_id === 'certificate-generate-self-signed'
  );
  assert(selfSignedCapability !== undefined, 'wasm self-signed canonical capability');
  assertEquals('wasm-browser', securityCapabilities.profiles[0], 'wasm capability profile');
  assertEquals('unsupported', selfSignedCapability.operational_state, 'wasm self-signed state');
  assertEquals('profile_unsupported', selfSignedCapability.reason_code, 'wasm self-signed reason');
  assertEquals('UNSUPPORTED', selfSignedCapability.stable_error, 'wasm self-signed stable error');
  try {
    security.certificate_generate_self_signed_json(
      'wasm-cert',
      ['localhost'],
      [],
      undefined,
      30,
      'p256',
      true
    );
    throw new Error('wasm self-signed unsupported: expected failure');
  } catch (error) {
    assert(String(error).includes('UNSUPPORTED'), 'wasm self-signed unsupported code');
    assert(
      String(error).includes('self-signed certificate generation is unavailable in WASM'),
      'wasm self-signed unsupported message'
    );
  }

  const path = `runtime-${crypto.randomUUID()}.loom`;
  let db;
  let root;
  try {
    db = await LoomSql.create(path, 'app', 'main', 'default', undefined, undefined);
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
    root = bootstrap.root;
    db.identity_set_passphrase(root, 'root-pass');
    try {
      db.identity_list_json();
      throw new Error('identity list before auth: expected failure');
    } catch (error) {
      assert(String(error).includes('AUTHENTICATION_FAILED'), 'identity list before auth');
    }
    try {
      db.identity_replicate_authority_json('denied-source', bytes('not identity'), false);
      throw new Error('authority replicate denied: expected failure');
    } catch (error) {
      assert(String(error).includes('AUTHENTICATION_FAILED'), 'authority replicate denied');
    }
    db.authenticate_passphrase(root, 'root-pass');
    try {
      db.identity_replicate_authority_json('malformed-source', bytes('not identity'), false);
      throw new Error('authority replicate malformed source: expected failure');
    } catch (error) {
      assert(String(error).includes('CORRUPT_OBJECT'), 'authority replicate malformed source');
    }
    const authorityKeyPair = await crypto.subtle.generateKey(
      { name: 'ECDSA', namedCurve: 'P-256' },
      true,
      ['sign', 'verify']
    );
    const authorityPublicKey = new Uint8Array(
      await crypto.subtle.exportKey('raw', authorityKeyPair.publicKey)
    );
    const authorityKey = db.identity_add_public_key(
      root,
      'authority-handoff',
      'ES256',
      hex(authorityPublicKey)
    );
    const authoritySnapshot = await signedFastForwardIdentitySnapshot(
      db.identity_authority_source_snapshot(),
      root,
      authorityKey,
      authorityKeyPair.privateKey
    );
    const replication = JSON.parse(
      db.identity_replicate_authority_json('self-snapshot', authoritySnapshot, false)
    );
    assertEquals(true, replication.applied, 'authority replicate canonical fast-forward');
    assertEquals(0, replication.from_generation, 'authority replicate from generation');
    assertEquals(1, replication.to_generation, 'authority replicate to generation');
    assertEquals(1, replication.witness.generation, 'authority replicate witness generation');
    assertEquals('mirror', replication.witness.mode, 'authority replicate witness mode');
    assert(replication.seq >= 0, 'authority replicate audit seq');
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
    db.workspace_create('lane-work', undefined);
    let lane = db.lanes_create('lane-work', laneCbor());
    assertEquals(0, laneTicketIds(lane).length, 'lane starts empty');
    lane = db.lanes_ticket_add(
      'lane-work',
      'agent-order',
      'MX-1',
      'agent-1',
      LaneTicketPlacement.Last,
      undefined
    );
    lane = db.lanes_ticket_add(
      'lane-work',
      'agent-order',
      'MX-2',
      'agent-1',
      LaneTicketPlacement.Last,
      undefined
    );
    lane = db.lanes_ticket_add(
      'lane-work',
      'agent-order',
      'MX-0',
      'agent-1',
      LaneTicketPlacement.First,
      undefined
    );
    lane = db.lanes_ticket_add(
      'lane-work',
      'agent-order',
      'MX-1.5',
      'agent-1',
      LaneTicketPlacement.After,
      'MX-1'
    );
    lane = db.lanes_ticket_add(
      'lane-work',
      'agent-order',
      'MX-0.5',
      'agent-1',
      LaneTicketPlacement.Before,
      'MX-1'
    );
    assertEquals(
      'MX-0,MX-0.5,MX-1,MX-1.5,MX-2',
      laneTicketIds(lane).join(','),
      'typed lane placement order'
    );
    lane = undefined;
    assert(db.commit('seed', 'wasm').startsWith('blake3:'), 'sql commit');
  } finally {
    if (db !== undefined) {
      db.free();
      db = undefined;
    }
  }
  const reopened = await LoomSql.open(path, 'app', 'main');
  try {
    reopened.authenticate_passphrase(root, 'root-pass');
    const reopenedAuthority = identityAuthorityState(reopened.identity_authority_source_snapshot());
    assertEquals(1, reopenedAuthority.generation, 'reopened authority generation');
    assertEquals(1, reopenedAuthority.mode, 'reopened authority mirror mode');
    assertRawBytes(uuidBytes(root), reopenedAuthority.authority, 'reopened authority principal');
    const audit = JSON.parse(reopened.audit_list_json());
    assert(
      audit.records.some((record) => record.action === 'identity.authority.replicate'),
      'authority replicate persisted audit'
    );
    const reopenedRows = reopened.query('SELECT id, v FROM t ORDER BY id');
    assertEquals(2, reopenedRows.length, 'sql reopen row count');
    assertEquals('1', reopenedRows[0][0].toString(), 'sql reopen first id');
    assertEquals('a', reopenedRows[0][1], 'sql reopen first value');
  } finally {
    reopened.free();
  }
};

run()
  .then(() => postMessage({ ok: true }))
  .catch((error) => postMessage({ ok: false, error: error?.stack || String(error) }));
