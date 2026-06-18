package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;

public final class LoomSession implements AutoCloseable {
    final String path;
    final String passphrase;
    private String authPrincipal;
    private String authPassphrase;

    LoomSession(String path, String passphrase) {
        this.path = path;
        this.passphrase = passphrase;
    }

    byte[] passphraseBytes() {
        return passphrase != null
                ? passphrase.getBytes(StandardCharsets.UTF_8)
                : null;
    }

    String authPrincipal() {
        return authPrincipal;
    }

    String authPassphrase() {
        return authPassphrase;
    }

    void setAuthentication(String principal, String principalPassphrase) {
        authPrincipal = principal;
        authPassphrase = principalPassphrase;
    }

    public void clearAuthentication() {
        authPrincipal = null;
        authPassphrase = null;
    }

    public byte[] execCbor(byte[] request) {
        return onHandle("loom_exec_cbor", (arena, handle) -> {
            MemorySegment req = arena.allocate(Math.max(request.length, 1));
            MemorySegment.copy(request, 0, req, ValueLayout.JAVA_BYTE, 0, request.length);
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_EXEC_CBOR.invokeExact(
                    handle, req, (long) request.length, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_exec_cbor");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    <T> T onHandle(String op, Loom.HandleOp<T> body) {
        return Loom.onHandle(path, passphraseBytes(), null, op, (arena, handle) -> {
            authenticateCurrentSession(arena, handle);
            return body.run(arena, handle);
        });
    }

    private void authenticateCurrentSession(Arena arena, MemorySegment handle) throws Throwable {
        if (authPrincipal == null) {
            return;
        }
        byte[] pass = authPassphrase.getBytes(StandardCharsets.UTF_8);
        MemorySegment passSeg = arena.allocate(Math.max(pass.length, 1));
        MemorySegment.copy(pass, 0, passSeg, ValueLayout.JAVA_BYTE, 0, pass.length);
        int status = (int) Loom.LOOM_AUTHENTICATE_PASSPHRASE.invokeExact(
                handle, arena.allocateFrom(authPrincipal), passSeg, (long) pass.length);
        if (status != 0) {
            throw Loom.lastError("loom_authenticate_passphrase");
        }
    }

    public KvOps kv() {
        return new KvOps(this);
    }

    public GraphOps graph() {
        return new GraphOps(this);
    }

    public VectorOps vector() {
        return new VectorOps(this);
    }

    public ColumnarOps columnar() {
        return new ColumnarOps(this);
    }

    public DataframeOps dataframe() {
        return new DataframeOps(this);
    }

    public SearchOps search() {
        return new SearchOps(this);
    }

    public CasOps cas() {
        return new CasOps(this);
    }

    public ArchiveOps archive() {
        return new ArchiveOps(this);
    }

    public MeetingsOps meetings() {
        return new MeetingsOps(this);
    }

    public DriveOps drive() {
        return new DriveOps(this);
    }

    public TicketsOps tickets() {
        return new TicketsOps(this);
    }

    public PagesOps pages() {
        return new PagesOps(this);
    }

    public LanesOps lanes() {
        return new LanesOps(this);
    }

    public ChatOps chat() {
        return new ChatOps(this);
    }

    public DocumentOps document() {
        return new DocumentOps(this);
    }

    public TimeSeriesOps timeSeries() {
        return new TimeSeriesOps(this);
    }

    public TelemetryOps telemetry() {
        return new TelemetryOps(this);
    }

    public LedgerOps ledger() {
        return new LedgerOps(this);
    }

    public QueueOps queue() {
        return new QueueOps(this);
    }

    public VcsOps vcs() {
        return new VcsOps(this);
    }

    public CalendarOps calendar() {
        return new CalendarOps(this);
    }

    public ContactsOps contacts() {
        return new ContactsOps(this);
    }

    public MailOps mail() {
        return new MailOps(this);
    }

    public SqlTableOps tables() {
        return new SqlTableOps(this);
    }

    public WorkspaceOps workspaces() {
        return new WorkspaceOps(this);
    }

    public IdentityOps identity() {
        return new IdentityOps(this);
    }

    public Loom.LoomSql sql(String workspace, String db) {
        if (authPrincipal != null) {
            return passphrase != null
                    ? Loom.LoomSql.openEncryptedAuthenticated(
                            path, workspace, db, passphrase, authPrincipal, authPassphrase)
                    : Loom.LoomSql.authenticated(path, workspace, db, authPrincipal, authPassphrase);
        }
        return passphrase != null
                ? new Loom.LoomSql(path, workspace, db, passphrase)
                : new Loom.LoomSql(path, workspace, db);
    }

    @Override
    public void close() {
    }
}
