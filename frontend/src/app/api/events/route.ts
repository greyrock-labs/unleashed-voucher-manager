import { NextRequest } from "next/server";
import { sseManager } from "@/utils/sseManager";
import { randomUUID } from "crypto";

export async function GET(request: NextRequest) {
  const clientId = randomUUID();

  const stream = new ReadableStream({
    start(controller) {
      sseManager.addClient(clientId, controller);

      // Send initial connection message
      const welcomeMessage = `data: ${JSON.stringify({
        type: "connected",
        clientId: clientId,
      })}\n\n`;

      controller.enqueue(welcomeMessage);

      // Clean up when connection closes
      request.signal.addEventListener("abort", () => {
        sseManager.removeClient(clientId);
      });
    },
    cancel() {
      sseManager.removeClient(clientId);
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      Connection: "keep-alive",
    },
  });
}
