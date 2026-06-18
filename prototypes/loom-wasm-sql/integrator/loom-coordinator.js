// The ONLY worker file an integrator (origin Y) hosts. A SharedWorker script must be same-origin to be
// shared across the integrator's tabs, so it can't be loaded directly from the vendor CDN (X). This
// one-line same-origin stub pulls the actual relay logic from X via importScripts (honours CORS), so
// the integrator hosts a trivial file and the logic still ships from X.
//
// Set this URL to your vendor CDN. For the local two-origin demo it is the X server on :8000.
importScripts("http://localhost:8000/coordinator-impl.js");
