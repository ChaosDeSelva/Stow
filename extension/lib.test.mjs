import test from "node:test";
import assert from "node:assert/strict";
import { parsePairingCode, endpoint, isHttpUrl, failureReason } from "./lib.js";

test("parsePairingCode accepts port:token and rejects junk", () => {
  const ok = parsePairingCode(" 47823:0123456789abcdef0123456789abcdef ");
  assert.deepEqual(ok, { port: 47823, token: "0123456789abcdef0123456789abcdef" });
  assert.equal(parsePairingCode("1@evil.example:0123456789abcdef0123456789abcdef"), null);
  assert.equal(parsePairingCode("47823:short"), null);
  assert.equal(parsePairingCode("70000:0123456789abcdef0123456789abcdef"), null);
  assert.equal(parsePairingCode(""), null);
});

test("endpoint builds a loopback URL and refuses bad ports", () => {
  assert.equal(endpoint(47823, "/capture"), "http://127.0.0.1:47823/capture");
  assert.throws(() => endpoint("1@evil.example", "/capture"));
});

test("isHttpUrl and failureReason", () => {
  assert.equal(isHttpUrl("https://a.com"), true);
  assert.equal(isHttpUrl("chrome://extensions"), false);
  assert.match(failureReason(401), /Pairing code/);
  assert.match(failureReason(new TypeError("fetch")), /isn't running/);
});
