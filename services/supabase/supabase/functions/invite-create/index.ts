import {
  handler,
  RejectedRequest,
  requireFields,
  requireInviteId,
  requireTimestamp,
} from "../_shared/http.ts";

Deno.serve(handler("invite-create", 70 * 1024, (body) => {
  requireFields(body, ["id", "expires_at", "ciphertext"]);
  requireInviteId(body.id);
  requireTimestamp(body.expires_at);
  if (
    typeof body.ciphertext !== "string" || body.ciphertext.length === 0 ||
    body.ciphertext.length >= 65536 ||
    !/^[A-Za-z0-9_-]+$/.test(body.ciphertext)
  ) throw new RejectedRequest();
}));
