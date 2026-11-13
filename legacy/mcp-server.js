// MCP-сервер плагина локальных доменов (stdio, JSON-RPC 2.0, без зависимостей).
// Даёт агентам инструменты: список/добавление/удаление доменов, проверка
// доступности домена через реверс-прокси, список занятых loopback-портов.
// Работает как тонкий клиент к API менеджера (127.0.0.1:8080).
// Подключение в MCP-клиенте (пример):
//   command: node, args: ["C:\\Users\\devcu\\wireproxy\\mcp-server.js"]
'use strict';
const http = require('http');
const readline = require('readline');

const API = { host: '127.0.0.1', port: 8080 };

function apiRequest(method, path, body) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? null : Buffer.from(JSON.stringify(body));
    const headers = {};
    if (data) {
      headers['Content-Type'] = 'application/json';
      headers['Content-Length'] = data.length;
      headers['X-WPC'] = '1';
    }
    const req = http.request({ host: API.host, port: API.port, method, path, headers }, (res) => {
      let out = '';
      res.setEncoding('utf8');
      res.on('data', (c) => { out += c; });
      res.on('end', () => {
        let parsed = null;
        try { parsed = JSON.parse(out); } catch { /* не JSON */ }
        if ((res.statusCode || 0) >= 400) {
          const msg = parsed && parsed.error ? parsed.error : 'HTTP ' + res.statusCode;
          return reject(new Error(msg));
        }
        resolve(parsed);
      });
    });
    req.on('error', (e) => reject(new Error('менеджер недоступен: ' + e.message)));
    if (data) req.write(data);
    req.end();
  });
}

const TOOLS = [
  {
    name: 'local_domains_list',
    description: 'Список локальных доменов (domain -> port) и порты реверс-прокси (http/https).',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'local_domains_add',
    description: 'Зарегистрировать локальный домен: запросы к http(s)://<domain> будут проксироваться на 127.0.0.1:<port>. Если домен уже есть — порт обновится.',
    inputSchema: {
      type: 'object',
      properties: {
        domain: { type: 'string', description: 'например app.localhost' },
        port: { type: 'number', description: 'локальный порт 1..65535' },
      },
      required: ['domain', 'port'],
    },
  },
  {
    name: 'local_domains_remove',
    description: 'Удалить локальный домен из конфигурации.',
    inputSchema: {
      type: 'object',
      properties: { domain: { type: 'string' } },
      required: ['domain'],
    },
  },
  {
    name: 'local_domains_probe',
    description: 'Проверить, что зарегистрированный домен отвечает: GET / через реверс-прокси. Возвращает HTTP-статус, время ответа и ошибку (если есть).',
    inputSchema: {
      type: 'object',
      properties: { domain: { type: 'string' } },
      required: ['domain'],
    },
  },
  {
    name: 'local_ports',
    description: 'Список слушающих loopback-портов (127.0.0.1 / ::1) — помогает найти порт dev-сервера.',
    inputSchema: { type: 'object', properties: {} },
  },
];

async function callTool(name, args) {
  switch (name) {
    case 'local_domains_list':
      return apiRequest('GET', '/api/local-domains');
    case 'local_domains_add': {
      const domain = String(args.domain || '').trim();
      const port = Number(args.port);
      if (!domain) throw new Error('нужен domain');
      return apiRequest('POST', '/api/local-domains', { action: 'add', domain, port });
    }
    case 'local_domains_remove': {
      const domain = String(args.domain || '').trim();
      if (!domain) throw new Error('нужен domain');
      return apiRequest('POST', '/api/local-domains', { action: 'remove', domain });
    }
    case 'local_domains_probe': {
      const domain = String(args.domain || '').trim();
      if (!domain) throw new Error('нужен domain');
      return apiRequest('POST', '/api/local-domains/probe', { domain });
    }
    case 'local_ports':
      return apiRequest('GET', '/api/local-ports');
    default:
      throw new Error('неизвестный инструмент: ' + name);
  }
}

function write(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function respond(id, result, error) {
  if (error) write({ jsonrpc: '2.0', id, error });
  else write({ jsonrpc: '2.0', id, result });
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });

rl.on('line', (line) => {
  const text = line.trim();
  if (!text) return;
  let msg;
  try { msg = JSON.parse(text); } catch { return; }
  const id = msg.id;
  const method = msg.method;

  (async () => {
    if (method === 'initialize') {
      return respond(id, {
        protocolVersion: '2024-11-05',
        capabilities: { tools: {} },
        serverInfo: { name: 'wireproxy-local-domains', version: '1.0.0' },
      });
    }
    if (method === 'notifications/initialized') return; // уведомление — без ответа
    if (method === 'tools/list') return respond(id, { tools: TOOLS });
    if (method === 'ping') return respond(id, {});
    if (method === 'tools/call') {
      try {
        const params = msg.params || {};
        const result = await callTool(params.name, params.arguments || {});
        return respond(id, { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] });
      } catch (e) {
        return respond(id, { content: [{ type: 'text', text: 'Ошибка: ' + e.message }], isError: true });
      }
    }
    if (id !== undefined) {
      return respond(id, null, { code: -32601, message: 'method not found: ' + method });
    }
  })().catch((e) => {
    if (id !== undefined) respond(id, null, { code: -32603, message: String(e && e.message ? e.message : e) });
  });
});
