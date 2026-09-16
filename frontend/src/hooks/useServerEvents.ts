import { useEffect, useRef, useCallback } from "react";

function getBackoffDelay(attempt: number, base = 1000, max = 30000) {
  const jitter = Math.random() * 0.3 + 0.85; // 85–115% random factor
  return Math.min(base * Math.pow(2, attempt), max) * jitter;
}

export function useServerEvents() {
  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttempts = useRef(0);
  const maxReconnectAttempts = 5;

  const connect = useCallback(() => {
    // Avoid reconnecting if already connecting/open
    if (
      eventSourceRef.current &&
      (eventSourceRef.current.readyState === EventSource.OPEN ||
        eventSourceRef.current.readyState === EventSource.CONNECTING)
    ) {
      return;
    }

    // Close existing connection if any
    eventSourceRef.current?.close();

    console.log("Setting up SSE connection...");
    eventSourceRef.current = new EventSource("/api/events");

    eventSourceRef.current.onopen = () => {
      console.log("SSE connection opened");
      reconnectAttempts.current = 0;
    };

    eventSourceRef.current.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        switch (data.type) {
          case "connected":
            console.log(`SSE connected with clientId: ${data.clientId}`);
            break;
          case "vouchersUpdated":
            window.dispatchEvent(new CustomEvent("vouchersUpdated"));
            break;
          default:
            console.warn("Unknown SSE event type:", data.type);
            break;
        }
      } catch (error) {
        console.error("Error parsing SSE data:", error);
      }
    };

    eventSourceRef.current.onerror = (_error) => {
      console.log("SSE connection error, attempting to reconnect...");

      // Close the current connection
      eventSourceRef.current?.close();

      // Only attempt to reconnect if we haven't exceeded max attempts
      if (reconnectAttempts.current < maxReconnectAttempts) {
        reconnectAttempts.current++;
        const delay = getBackoffDelay(reconnectAttempts.current);

        console.log(
          `Reconnecting in ${Math.round(delay)}ms (attempt ${reconnectAttempts.current}/${maxReconnectAttempts})`,
        );

        reconnectTimeoutRef.current = setTimeout(connect, delay);
      } else {
        console.error("Max reconnection attempts reached, giving up");
      }
    };
  }, []);

  useEffect(() => {
    connect();

    return () => {
      // Clear any pending reconnection attempts
      reconnectTimeoutRef.current && clearTimeout(reconnectTimeoutRef.current);

      // Close the connection
      eventSourceRef.current?.close();
    };
  }, [connect]);

  return eventSourceRef.current;
}
