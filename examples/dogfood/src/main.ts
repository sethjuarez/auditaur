import { initAuditaur, type AuditaurClient } from '@auditaur/api';
import './styles.css';

const FRONTEND_EVENT = 'dogfood:frontend-event';
const BACKEND_EVENT = 'dogfood:backend-event';

const output = document.querySelector<HTMLPreElement>('#output');
const driveInput = document.querySelector<HTMLInputElement>('#drive-input');
const driveTextarea = document.querySelector<HTMLTextAreaElement>('#drive-textarea');
let client: AuditaurClient | undefined;

function write(message: string) {
  const timestamp = new Date().toLocaleTimeString();
  if (output) {
    output.textContent = `[${timestamp}] ${message}\n${output.textContent ?? ''}`;
  }
}

function button(id: string, handler: () => Promise<void> | void) {
  document.querySelector<HTMLButtonElement>(`#${id}`)?.addEventListener('click', async () => {
    try {
      await handler();
      await client?.flush();
    } catch (error) {
      write(error instanceof Error ? error.message : String(error));
      await client?.flush();
    }
  });
}

async function main() {
  client = await initAuditaur({
    serviceName: 'auditaur-dogfood-frontend',
    instrumentConsole: true,
    instrumentErrors: true,
    instrumentTauriInvoke: true,
    instrumentTauriEvents: true,
    captureFullPayloads: true,
    batchIntervalMs: 500,
    driveBridge: {
      windowLabel: 'main',
    },
  });

  await client.listen(BACKEND_EVENT, (event) => {
    write(`Received backend event: ${JSON.stringify(event.payload)}`);
  });
  await client.listen(FRONTEND_EVENT, (event) => {
    write(`Received frontend event: ${JSON.stringify(event.payload)}`);
  });

  button('console-log', () => {
    console.log('Dogfood console log', {
      source: 'frontend',
      secret: 'this value should be redacted by default',
    });
    write('Console log emitted.');
  });

  button('frontend-error', () => {
    write('Throwing frontend error on the next tick.');
    setTimeout(() => {
      throw new Error('Intentional dogfood frontend error');
    }, 0);
    setTimeout(() => {
      void client?.flush();
    }, 50);
  });

  button('frontend-event', async () => {
    await client?.emit(FRONTEND_EVENT, {
      source: 'frontend',
      message: 'hello from the webview',
    });
    write('Frontend event emitted.');
  });

  button('successful-command', async () => {
    const response = await client?.invoke<string>('successful_command', {
      message: 'hello from Auditaur',
    });
    write(`Successful command returned: ${response}`);
  });

  button('failing-command', async () => {
    await client?.invoke('failing_command', {
      reason: 'the dogfood button requested a failure',
    });
  });

  button('backend-event', async () => {
    await client?.invoke('emit_backend_event');
    write('Requested backend event.');
  });

  driveInput?.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') {
      write(`Drive input Enter pressed with value: ${driveInput.value}`);
    }
  });
  driveTextarea?.addEventListener('input', () => {
    write(`Drive textarea input: ${driveTextarea.value}`);
  });

  window.addEventListener('pagehide', () => {
    void client?.flush();
  });

  write('Auditaur initialized. Click a button to emit telemetry.');
}

main().catch((error) => {
  write(error instanceof Error ? error.message : String(error));
});
