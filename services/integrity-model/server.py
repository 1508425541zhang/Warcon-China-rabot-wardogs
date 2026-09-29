"""Standalone authenticated HTTP inference process. Does not connect to a game or database."""
import hmac
import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer
from inference import Predictor

MAX_BODY = 8 * 1024 * 1024


def make_handler(predictor, token):
    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(15)

        def log_message(self, *_):
            pass

        def reply(self, status, body):
            data = json.dumps(body, allow_nan=False).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(data)

        def authorized(self):
            return hmac.compare_digest(self.headers.get('Authorization', '').encode(), ('Bearer ' + token).encode())

        def do_GET(self):
            if not self.authorized():
                return self.reply(401, {'error': 'Unauthorized'})
            if self.path != '/v1/health':
                return self.reply(404, {'error': 'Not found'})
            self.reply(200, {'ok': True, **predictor.manifest})

        def do_POST(self):
            if not self.authorized():
                return self.reply(401, {'error': 'Unauthorized'})
            if self.path != '/v1/assess':
                return self.reply(404, {'error': 'Not found'})
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= MAX_BODY or self.headers.get('Transfer-Encoding'):
                    return self.reply(413, {'error': 'Invalid body size'})
                data = self.rfile.read(length)
                if len(data) != length:
                    raise ValueError('Incomplete request')
                body = json.loads(data, parse_constant=lambda _: (_ for _ in ()).throw(ValueError('Nonfinite JSON')))
                if not isinstance(body, dict):
                    raise ValueError('Object required')
                self.reply(200, predictor.assess(body))
            except (ValueError, TypeError, KeyError, OverflowError):
                self.reply(400, {'error': 'Invalid inference input'})
            except Exception:
                self.reply(500, {'error': 'Inference failed'})
    return Handler


if __name__ == '__main__':
    token = os.environ.get('MODEL_API_TOKEN', '')
    if len(token) < 32:
        raise SystemExit('MODEL_API_TOKEN must contain at least 32 characters')
    HTTPServer((os.environ.get('MODEL_HOST', '127.0.0.1'), int(os.environ.get('MODEL_PORT', '8091'))),
               make_handler(Predictor(), token)).serve_forever()
