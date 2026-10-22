import {
  handler,
  RejectedRequest,
  requireDecimal,
  requireFields,
} from "../_shared/http.ts";
Deno.serve(handler("project-claim", 4096, (body) => {
  requireFields(body, ["actor_seq"]);
  if (requireDecimal(body.actor_seq) !== "1") throw new RejectedRequest();
}));
