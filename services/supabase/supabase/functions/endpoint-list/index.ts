import { handler, RejectedRequest, requireFields } from "../_shared/http.ts";
Deno.serve(handler("endpoint-list", 4096, (body) => {
  requireFields(body, ["after_cursor"]);
  if (
    body.after_cursor !== null &&
    (typeof body.after_cursor !== "string" ||
      !/^(0|[1-9][0-9]{0,18})$/.test(body.after_cursor))
  ) throw new RejectedRequest();
}));
