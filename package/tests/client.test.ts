// The client calls a route of the contract with the token, and reads its answer as the contract's type.

import { expect, test } from "bun:test";

import { type Item, type JobFetch, jobClient } from "bmo-job-client";

const item: Item = {
  id: 7,
  title: "parser",
  brief: "write it",
  state: "open",
  queueName: "build",
  position: 1,
  priority: 0,
  assigneeName: null,
  requesterName: null,
  scope: null,
  labels: {},
  approvals: [{ kind: "approval", callerName: "qa-admin", at: 1 }],
  rejections: 0,
  mergedInto: null,
  raisedBy: null,
  leaseId: null,
  createdAt: 1,
  changedAt: 1,
  endedAt: null,
};

test("a call reaches its route with the token and reads the answer", async () => {
  const asked: Request[] = [];
  const fetch: JobFetch = async (input) => {
    asked.push(input);
    return new Response(JSON.stringify(item), { headers: { "Content-Type": "application/json" } });
  };
  const client = jobClient("http://job.test", "the-token", fetch);
  const { data, error } = await client.GET("/items/{itemId}", { params: { path: { itemId: 7 } } });
  expect(error).toBeUndefined();
  expect(data?.state).toBe("open");
  expect(asked[0].url).toBe("http://job.test/items/7");
  expect(asked[0].headers.get("Authorization")).toBe("Bearer the-token");
});

test("a refusal is read in the error shape", async () => {
  const fetch: JobFetch = async () =>
    new Response(JSON.stringify({ code: "not-found", error: "There is no item 9." }), {
      status: 404,
      headers: { "Content-Type": "application/json" },
    });
  const client = jobClient("http://job.test", "the-token", fetch);
  const { data, error } = await client.GET("/items/{itemId}", { params: { path: { itemId: 9 } } });
  expect(data).toBeUndefined();
  expect(error?.code).toBe("not-found");
});
