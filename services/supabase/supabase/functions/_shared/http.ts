export type Operation =
  | "project-claim"
  | "member-grant"
  | "member-revoke"
  | "endpoint-publish"
  | "endpoint-list"
  | "head-publish"
  | "head-list"
  | "invite-create";

type Auth = {
  version: number;
  project_route: string;
  device_id: string;
  public_key: string;
  issued_at: string;
  nonce: string;
  scope: string;
  body_hash: string;
  signature: string;
};

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const NONCE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const HEX_32 = /^[0-9a-f]{64}$/;
const HEX_64 = /^[0-9a-f]{128}$/;
const RFC3339_MILLIS = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;
const INVITE_ID = /^[A-Za-z0-9_-]{43}$/;
const AUTH_FIELDS = [
  "version",
  "project_route",
  "device_id",
  "public_key",
  "issued_at",
  "nonce",
  "scope",
  "body_hash",
  "signature",
];

export class RejectedRequest extends Error {}
class BackendConfigurationError extends Error {}

function serviceSecretKey(): string {
  const source = Deno.env.get("SUPABASE_SECRET_KEYS");
  let keys: unknown;
  try {
    keys = source ? JSON.parse(source) : null;
  } catch {
    throw new BackendConfigurationError();
  }
  if (typeof keys !== "object" || keys === null || Array.isArray(keys)) {
    throw new BackendConfigurationError();
  }
  const key = (keys as Record<string, unknown>).default;
  if (
    typeof key !== "string" || !key.startsWith("sb_secret_") ||
    key.length < "sb_secret_".length + 16 || key.length > 256 ||
    !/^[A-Za-z0-9_-]+$/.test(key)
  ) throw new BackendConfigurationError();
  return key;
}

export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      "cache-control": "no-store",
    },
  });
}

function bytesFromHex(value: string): Uint8Array<ArrayBuffer> {
  const out: Uint8Array<ArrayBuffer> = new Uint8Array(value.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = Number.parseInt(value.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesToHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function parseAuth(header: string | null): Auth {
  if (!header || header.length > 4096) throw new RejectedRequest();
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(
      Uint8Array.from(atob(header), (char) => char.charCodeAt(0)),
    ));
  } catch {
    throw new RejectedRequest();
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new RejectedRequest();
  }
  const value = parsed as Record<string, unknown>;
  if (
    Object.keys(value).length !== AUTH_FIELDS.length ||
    AUTH_FIELDS.some((field) => !(field in value))
  ) {
    throw new RejectedRequest();
  }
  const auth = value as unknown as Auth;
  if (
    auth.version !== 1 ||
    typeof auth.project_route !== "string" || !UUID.test(auth.project_route) ||
    auth.project_route === "00000000-0000-0000-0000-000000000000" ||
    typeof auth.device_id !== "string" || !UUID.test(auth.device_id) ||
    auth.device_id === "00000000-0000-0000-0000-000000000000" ||
    typeof auth.nonce !== "string" || !NONCE.test(auth.nonce) ||
    typeof auth.public_key !== "string" || !HEX_32.test(auth.public_key) ||
    typeof auth.signature !== "string" || !HEX_64.test(auth.signature) ||
    typeof auth.body_hash !== "string" || !HEX_32.test(auth.body_hash) ||
    typeof auth.issued_at !== "string" ||
    !RFC3339_MILLIS.test(auth.issued_at) ||
    typeof auth.scope !== "string"
  ) {
    throw new RejectedRequest();
  }
  const parsedTime = Date.parse(auth.issued_at);
  if (
    !Number.isFinite(parsedTime) ||
    new Date(parsedTime).toISOString() !== auth.issued_at ||
    Math.abs(Date.now() - parsedTime) > 120_000
  ) throw new RejectedRequest();
  return auth;
}

async function verifyAuth(
  request: Request,
  operation: Operation,
  rawBody: Uint8Array<ArrayBuffer>,
): Promise<Auth> {
  const auth = parseAuth(request.headers.get("x-trackmesh-auth"));
  if (auth.scope !== operation) throw new RejectedRequest();
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", rawBody));
  if (bytesToHex(digest) !== auth.body_hash) throw new RejectedRequest();

  // Mirrors MailboxAuth's serde_json tuple signing model with a new domain.
  const signingTuple = JSON.stringify([
    "trackmesh-supabase-request-v1",
    auth.version,
    auth.project_route,
    auth.device_id,
    auth.public_key,
    auth.issued_at,
    auth.nonce,
    auth.scope,
    auth.body_hash,
  ]);
  try {
    const key = await crypto.subtle.importKey(
      "raw",
      bytesFromHex(auth.public_key),
      { name: "Ed25519" },
      false,
      ["verify"],
    );
    const signingBytes: Uint8Array<ArrayBuffer> = new TextEncoder().encode(
      signingTuple,
    );
    if (
      !await crypto.subtle.verify(
        { name: "Ed25519" },
        key,
        bytesFromHex(auth.signature),
        signingBytes,
      )
    ) throw new RejectedRequest();
  } catch {
    throw new RejectedRequest();
  }
  return auth;
}

export async function readBoundedBody(
  request: Request,
  maxBytes: number,
): Promise<Uint8Array<ArrayBuffer>> {
  const contentLength = request.headers.get("content-length");
  if (
    contentLength &&
    (!/^\d+$/.test(contentLength) || Number(contentLength) > maxBytes)
  ) {
    throw new RejectedRequest();
  }
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const part = await reader.read();
    if (part.done) break;
    total += part.value.byteLength;
    if (total > maxBytes) {
      await reader.cancel();
      throw new RejectedRequest();
    }
    chunks.push(part.value);
  }
  const body: Uint8Array<ArrayBuffer> = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body;
}

export function parseBody(
  raw: Uint8Array<ArrayBuffer>,
): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(raw));
  } catch {
    throw new RejectedRequest();
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new RejectedRequest();
  }
  return value as Record<string, unknown>;
}

export function requireFields(
  body: Record<string, unknown>,
  fields: string[],
): void {
  if (
    Object.keys(body).length !== fields.length ||
    fields.some((field) => !(field in body))
  ) {
    throw new RejectedRequest();
  }
}

export function requireDecimal(value: unknown, maxDigits = 20): string {
  if (
    typeof value !== "string" || value.length > maxDigits ||
    !/^[1-9][0-9]*$/.test(value)
  ) {
    throw new RejectedRequest();
  }
  if (BigInt(value) > 18_446_744_073_709_551_615n) throw new RejectedRequest();
  return value;
}

export function requireUuid(value: unknown): string {
  if (
    typeof value !== "string" || !UUID.test(value) ||
    value === "00000000-0000-0000-0000-000000000000"
  ) {
    throw new RejectedRequest();
  }
  return value;
}

export function requireInviteId(value: unknown): string {
  if (typeof value !== "string" || !INVITE_ID.test(value)) {
    throw new RejectedRequest();
  }
  return value;
}

export function requirePublicKey(value: unknown): string {
  if (typeof value !== "string" || !HEX_32.test(value)) {
    throw new RejectedRequest();
  }
  return value;
}

export function requireTimestamp(value: unknown): string {
  if (
    typeof value !== "string" || !RFC3339_MILLIS.test(value) ||
    !Number.isFinite(Date.parse(value)) ||
    new Date(Date.parse(value)).toISOString() !== value
  ) {
    throw new RejectedRequest();
  }
  return value;
}

export async function dispatch(
  operation: Operation,
  body: Record<string, unknown>,
  auth: Auth,
): Promise<unknown> {
  const projectUrl = Deno.env.get("SUPABASE_URL");
  const secretKey = serviceSecretKey();
  if (!projectUrl) throw new Error("backend unavailable");

  let response: Response;
  const rpcName = operation === "invite-create"
    ? "trackmesh_invite_create"
    : "trackmesh_dispatch";
  const rpcBody = operation === "invite-create"
    ? {
      p_project_route: auth.project_route,
      p_device_id: auth.device_id,
      p_public_key: auth.public_key,
      p_issued_at: auth.issued_at,
      p_nonce: auth.nonce,
      p_id: body.id,
      p_expires_at: body.expires_at,
      p_ciphertext: body.ciphertext,
    }
    : {
      p_operation: operation,
      p_project_route: auth.project_route,
      p_device_id: auth.device_id,
      p_public_key: auth.public_key,
      p_issued_at: auth.issued_at,
      p_nonce: auth.nonce,
      p_body_hash: auth.body_hash,
      p_signature: auth.signature,
      p_body: body,
    };
  try {
    response = await fetch(
      projectUrl.replace(/\/$/, "") + "/rest/v1/rpc/" + rpcName,
      {
        method: "POST",
        headers: {
          apikey: secretKey,
          "content-type": "application/json",
        },
        signal: AbortSignal.timeout(10_000),
        body: JSON.stringify(rpcBody),
      },
    );
  } catch {
    throw new Error("backend unavailable");
  }
  if (
    response.status >= 500 || response.status === 429 ||
    response.status === 401 || response.status === 403
  ) {
    throw new Error("backend unavailable");
  }
  if (!response.ok) throw new RejectedRequest();
  let result: unknown;
  try {
    result = await response.json();
  } catch {
    throw new Error("backend unavailable");
  }
  if (
    typeof result === "object" && result !== null && !Array.isArray(result) &&
    "accepted" in result &&
    (result as { accepted?: unknown }).accepted === false
  ) {
    throw new RejectedRequest();
  }
  return result;
}

export function handler(
  operation: Operation,
  maxBytes: number,
  validate: (body: Record<string, unknown>) => void,
) {
  return async (request: Request): Promise<Response> => {
    try {
      if (request.method !== "POST") throw new RejectedRequest();
      const rawBody = await readBoundedBody(request, maxBytes);
      const auth = await verifyAuth(request, operation, rawBody);
      const body = parseBody(rawBody);
      validate(body);
      return jsonResponse(await dispatch(operation, body, auth));
    } catch (error) {
      if (error instanceof RejectedRequest) {
        return jsonResponse({ error: "request rejected" }, 400);
      }
      if (error instanceof BackendConfigurationError) {
        return jsonResponse({ error: "service unavailable" }, 500);
      }
      return jsonResponse({ error: "service unavailable" }, 503);
    }
  };
}
