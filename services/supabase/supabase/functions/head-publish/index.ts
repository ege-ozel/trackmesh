import { handler, RejectedRequest, requireDecimal, requireFields } from "../_shared/http.ts";
Deno.serve(handler("head-publish", 8192, (body) => {
  requireFields(body, ["seq", "ciphertext"]);
  requireDecimal(body.seq);
  if (typeof body.ciphertext !== "string" || body.ciphertext.length === 0 || body.ciphertext.length > 4096 || !/^[A-Za-z0-9_-]+$/.test(body.ciphertext)) throw new RejectedRequest();
}));
