import {
  handler,
  requireDecimal,
  requireFields,
  requireUuid,
} from "../_shared/http.ts";
Deno.serve(handler("member-revoke", 4096, (body) => {
  requireFields(body, ["actor_seq", "target_device_id"]);
  requireDecimal(body.actor_seq);
  requireUuid(body.target_device_id);
}));
