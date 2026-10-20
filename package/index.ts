// The job service's contract in TypeScript (docs/api.md, "The client"): the types openapi-typescript generates
// from docs/openapi.json, and a client over them that carries the token on every call.

import createClient, { type Client } from "openapi-fetch";

import type { components, operations, paths } from "./openapi";

export type { components, operations, paths };

/** The fetch a client asks: one request, one answer. `globalThis.fetch` is one. */
export type JobFetch = (input: Request) => Promise<Response>;

type Schemas = components["schemas"];

export type Item = Schemas["Item"];
export type ItemState = Schemas["ItemState"];
export type Note = Schemas["Note"];
export type Report = Schemas["Report"];
export type WorkOrder = Schemas["WorkOrder"];
export type Task = Schemas["Task"];
export type Queue = Schemas["Queue"];
export type Lease = Schemas["Lease"];
export type Question = Schemas["Question"];
export type Reply = Schemas["Reply"];
export type Subscription = Schemas["Subscription"];
export type ScopeHolding = Schemas["ScopeHolding"];
export type Event = Schemas["Event"];
export type ErrorBody = Schemas["Error"];

/**
 * A client of the job service at `baseUrl`, sending `token` as its bearer token. Each call is a route of the
 * OpenAPI document, typed by it.
 */
export function jobClient(baseUrl: string, token: string, fetch?: JobFetch): Client<paths> {
  return createClient<paths>({
    baseUrl,
    headers: { Authorization: `Bearer ${token}` },
    ...(fetch ? { fetch } : {}),
  });
}
