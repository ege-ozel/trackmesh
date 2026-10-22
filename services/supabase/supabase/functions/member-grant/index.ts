import {
  handler,
  requireDecimal,
  requireFields,
  requirePublicKey,
  requireUuid,
} from "../_shared/http.ts";
Deno.serve(handler("member-grant", 4096, (body) => {
  requireFields(body, ["actor_seq", "target_device_id", "target_public_key"]);
  requireDecimal(body.actor_seq);
  requireUuid(body.target_device_id);
  requirePublicKey(body.target_public_key);
}));
