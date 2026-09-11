import { api, type AgentStatus } from "./ipc";

let cached: AgentStatus | null = null;
let inflight: Promise<AgentStatus> | null = null;

/** 上次检测结果；没有则尚未查过。 */
export function peekAgentStatus(): AgentStatus | null {
  return cached;
}

export function rememberAgentStatus(status: AgentStatus | null) {
  cached = status;
}

export function forgetAgentStatus() {
  cached = null;
  inflight = null;
}

/** 有缓存则直接返回；并发请求合并成一次。`force` 会重新探测。 */
export function loadAgentStatus(force = false): Promise<AgentStatus> {
  if (inflight) return inflight;
  if (!force && cached) return Promise.resolve(cached);
  const req = api
    .agentStatus()
    .then((s) => {
      cached = s;
      return s;
    })
    .finally(() => {
      if (inflight === req) inflight = null;
    });
  inflight = req;
  return req;
}
