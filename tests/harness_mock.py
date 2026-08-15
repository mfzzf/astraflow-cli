#!/usr/bin/env python3
import json
import os
import socket
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

LOG = os.environ["HARNESS_MOCK_LOG"]
MODEL = os.environ.get("HARNESS_MOCK_MODEL", "astraflow-test-model")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        return

    def _json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            pass

    def do_GET(self):
        path = urlparse(self.path).path.rstrip("/")
        if path == "/v1/models":
            self._json(200, {"object": "list", "data": [{"id": MODEL, "object": "model"}]})
        else:
            self._json(404, {"error": {"message": "not found"}})

    def do_POST(self):
        path = urlparse(self.path).path.rstrip("/")
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length)
        try:
            payload = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            payload = {}
        record = {
            "path": self.path,
            "authorization": self.headers.get("authorization"),
            "x_api_key": self.headers.get("x-api-key"),
            "model": payload.get("model"),
            "headers": {key.lower(): value for key, value in self.headers.items()},
        }
        with open(LOG, "a", encoding="utf-8") as output:
            output.write(json.dumps(record) + "\n")

        if path == "/v1/messages":
            self._anthropic(payload)
        elif path in ("/v1/responses", "/v1/response"):
            self._responses(payload)
        elif path == "/v1/chat/completions":
            self._chat(payload)
        else:
            self._json(404, {"error": {"message": "not found"}})

    def _sse(self, events):
        body = "".join(f"event: {name}\ndata: {json.dumps(data)}\n\n" for name, data in events).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            pass

    def _chat(self, payload):
        if payload.get("stream"):
            chunks = [
                {"id": "chatcmpl-test", "object": "chat.completion.chunk", "created": 0, "model": MODEL,
                 "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ASTRAFLOW_OK"}, "finish_reason": None}]},
                {"id": "chatcmpl-test", "object": "chat.completion.chunk", "created": 0, "model": MODEL,
                 "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                 "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}},
            ]
            body = "".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n"
            encoded = body.encode()
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(encoded)))
            self.end_headers()
            try:
                self.wfile.write(encoded)
            except (BrokenPipeError, ConnectionResetError, socket.timeout):
                pass
        else:
            self._json(200, {
                "id": "chatcmpl-test", "object": "chat.completion", "created": 0, "model": MODEL,
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ASTRAFLOW_OK"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })

    def _responses(self, payload):
        response = {
            "id": "resp_test", "object": "response", "status": "completed", "model": MODEL,
            "output": [{"id": "msg_test", "type": "message", "status": "completed", "role": "assistant",
                        "content": [{"type": "output_text", "text": "ASTRAFLOW_OK", "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
        }
        if payload.get("stream"):
            item = response["output"][0]
            part = item["content"][0]
            self._sse([
                ("response.created", {"type": "response.created", "response": {**response, "status": "in_progress", "output": []}}),
                ("response.output_item.added", {"type": "response.output_item.added", "output_index": 0, "item": {**item, "status": "in_progress", "content": []}}),
                ("response.content_part.added", {"type": "response.content_part.added", "item_id": "msg_test", "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}}),
                ("response.output_text.delta", {"type": "response.output_text.delta", "item_id": "msg_test", "output_index": 0, "content_index": 0, "delta": "ASTRAFLOW_OK"}),
                ("response.output_text.done", {"type": "response.output_text.done", "item_id": "msg_test", "output_index": 0, "content_index": 0, "text": "ASTRAFLOW_OK"}),
                ("response.content_part.done", {"type": "response.content_part.done", "item_id": "msg_test", "output_index": 0, "content_index": 0, "part": part}),
                ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0, "item": item}),
                ("response.completed", {"type": "response.completed", "response": response}),
            ])
        else:
            self._json(200, response)

    def _anthropic(self, payload):
        if payload.get("stream"):
            self._sse([
                ("message_start", {"type": "message_start", "message": {"id": "msg_test", "type": "message", "role": "assistant", "model": MODEL, "content": [], "stop_reason": None, "usage": {"input_tokens": 1, "output_tokens": 0}}}),
                ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ASTRAFLOW_OK"}}),
                ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 1}}),
                ("message_stop", {"type": "message_stop"}),
            ])
        else:
            self._json(200, {"id": "msg_test", "type": "message", "role": "assistant", "model": MODEL,
                             "content": [{"type": "text", "text": "ASTRAFLOW_OK"}], "stop_reason": "end_turn",
                             "usage": {"input_tokens": 1, "output_tokens": 1}})


if __name__ == "__main__":
    address = os.environ.get("HARNESS_MOCK_ADDRESS", "127.0.0.1:18080")
    host, port = address.rsplit(":", 1)
    ThreadingHTTPServer((host, int(port)), Handler).serve_forever()
