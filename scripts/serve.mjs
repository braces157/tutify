import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { pipeline } from 'node:stream';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const docsRoot = path.join(projectRoot, 'docs');
const docsRootPrefix = `${docsRoot}${path.sep}`;
const portArgIndex = process.argv.indexOf('--port');
const port = Number(portArgIndex >= 0 ? process.argv[portArgIndex + 1] : process.env.PORT || 4173);
const mimeTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
};

function sendText(response, statusCode, body) {
  if (response.destroyed || response.writableEnded) {
    return;
  }
  response.writeHead(statusCode, { 'Content-Type': 'text/plain; charset=utf-8' });
  response.end(body);
}

function resolveRequestPath(requestUrl) {
  const rawPath = (requestUrl || '/').split('?')[0];
  if (!rawPath.startsWith('/')) {
    return { error: 400, message: 'Bad request' };
  }

  let requestPath;
  try {
    requestPath = decodeURIComponent(rawPath);
  } catch {
    return { error: 400, message: 'Bad request' };
  }

  if (requestPath.includes('\0')) {
    return { error: 400, message: 'Bad request' };
  }

  // Serve the docs directory for both the canonical /docs/... URLs and the
  // root-relative asset URLs used by the page served at /.
  const docsRelativePath = requestPath === '/'
    ? '/index.html'
    : requestPath.startsWith('/docs/')
      ? requestPath.slice('/docs'.length)
      : requestPath;
  const filePath = path.resolve(docsRoot, `.${docsRelativePath}`);
  if (filePath !== docsRoot && !filePath.startsWith(docsRootPrefix)) {
    return { error: 403, message: 'Forbidden' };
  }

  return { filePath };
}

const server = http.createServer((request, response) => {
  const resolvedPath = resolveRequestPath(request.url);
  if (resolvedPath.error) {
    sendText(response, resolvedPath.error, resolvedPath.message);
    return;
  }

  const { filePath } = resolvedPath;
  fs.stat(filePath, (error, stat) => {
    if (error) {
      sendText(response, error.code === 'EACCES' ? 403 : 404, error.code === 'EACCES' ? 'Forbidden' : 'Not found');
      return;
    }
    if (!stat.isFile()) {
      sendText(response, 404, 'Not found');
      return;
    }

    if (response.destroyed || response.writableEnded) {
      return;
    }
    response.writeHead(200, {
      'Cache-Control': 'no-cache',
      'Content-Type': mimeTypes[path.extname(filePath).toLowerCase()] || 'application/octet-stream',
    });

    // pipeline handles read errors and client disconnects without leaving an
    // unhandled stream error that would terminate the local server process.
    pipeline(fs.createReadStream(filePath), response, (streamError) => {
      if (streamError && !response.destroyed) {
        response.destroy();
      }
    });
  });
});

server.listen(port, '127.0.0.1', () => {
  console.log(`Tuitify website server listening on http://127.0.0.1:${port}`);
});
