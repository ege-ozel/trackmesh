import {
  jsonResponse,
  parseBody,
  readBoundedBody,
  RejectedRequest,
  requireFields,
  requireInviteId,
} from "../_shared/http.ts";

Deno.serve(async (request) => {
  try {
    if (request.method !== "POST") throw new RejectedRequest();
    const raw = await readBoundedBody(request, 1024);
    const body = parseBody(raw);
    requireFields(body, ["id"]);
    const id = requireInviteId(body.id);
    const projectUrl = Deno.env.get("SUPABASE_URL");
    const secretsSource = Deno.env.get("SUPABASE_SECRET_KEYS");
    let secrets: unknown;
    try {
      secrets = secretsSource ? JSON.parse(secretsSource) : null;
    } catch {
      return jsonResponse({ error: "service unavailable" }, 500);
    }
    const serviceSecretKey =
      typeof secrets === "object" && secrets !== null && !Array.isArray(secrets)
        ? (secrets as Record<string, unknown>).default
        : null;
    if (
      typeof serviceSecretKey !== "string" ||
      !serviceSecretKey.startsWith("sb_secret_") ||
      serviceSecretKey.length < "sb_secret_".length + 16 ||
      serviceSecretKey.length > 256 ||
      !/^[A-Za-z0-9_-]+$/.test(serviceSecretKey)
    ) return jsonResponse({ error: "service unavailable" }, 500);
    if (!projectUrl) return jsonResponse({ error: "service unavailable" }, 503);
    const response = await fetch(
      projectUrl.replace(/\/$/, "") +
        "/rest/v1/rpc/trackmesh_invite_consume",
      {
        method: "POST",
        headers: {
          apikey: serviceSecretKey,
          "content-type": "application/json",
        },
        body: JSON.stringify({ p_id: id }),
        signal: AbortSignal.timeout(10_000),
      },
    );
    if (!response.ok) return jsonResponse({ error: "request rejected" }, 400);
    const result: unknown = await response.json();
    if (
      typeof result !== "object" || result === null || Array.isArray(result) ||
      !("accepted" in result) || result.accepted !== true ||
      !("ciphertext" in result) || typeof result.ciphertext !== "string" ||
      !("expires_at" in result) || typeof result.expires_at !== "string"
    ) return jsonResponse({ error: "request rejected" }, 400);
    return jsonResponse({
      ciphertext: result.ciphertext,
      expires_at: result.expires_at,
    });
  } catch (error) {
    if (error instanceof RejectedRequest) {
      return jsonResponse({ error: "request rejected" }, 400);
    }
    return jsonResponse({ error: "service unavailable" }, 503);
  }
});
