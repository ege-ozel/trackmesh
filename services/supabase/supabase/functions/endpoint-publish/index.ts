import {
  handler,
  RejectedRequest,
  requireDecimal,
  requireFields,
  requireTimestamp,
} from "../_shared/http.ts";
Deno.serve(handler("endpoint-publish", 64 * 1024, (body) => {
  requireFields(body, ["seq", "expires_at", "ciphertext"]);
  requireDecimal(body.seq);
  requireTimestamp(body.expires_at);
  if (
    typeof body.ciphertext !== "string" || body.ciphertext.length === 0 ||
    body.ciphertext.length > 64 * 1024 ||
    !/^[A-Za-z0-9_-]+$/.test(body.ciphertext)
  ) throw new RejectedRequest();
}));
