import { handler, requireFields } from "../_shared/http.ts";
Deno.serve(handler("head-list", 4096, (body) => requireFields(body, [])));
