class SSEManager {
  private clients: Map<string, ReadableStreamDefaultController> = new Map();

  addClient(id: string, controller: ReadableStreamDefaultController) {
    this.clients.set(id, controller);
    console.log(`Client ${id} added. Total clients: ${this.clients.size}`);
  }

  removeClient(id: string) {
    const removed = this.clients.delete(id);
    console.log(
      `Client ${id} removed: ${removed}. Total clients: ${this.clients.size}`,
    );
  }

  broadcastToClients(data: any) {
    const message = `data: ${JSON.stringify(data)}\n\n`;

    const clientsToRemove: string[] = [];

    this.clients.forEach((controller, id) => {
      try {
        controller.enqueue(message);
      } catch (error) {
        console.error(
          `Error sending message to client ${id}, marking for removal:`,
          error,
        );
        clientsToRemove.push(id);
      }
    });

    // Clean up dead connections
    clientsToRemove.forEach((id) => this.clients.delete(id));

    if (clientsToRemove.length > 0) {
      console.log(
        `Removed ${clientsToRemove.length} dead connections. Remaining: ${this.clients.size}`,
      );
    }
  }

  getClientCount() {
    return this.clients.size;
  }

  getAllClientIds() {
    return Array.from(this.clients.keys());
  }
}

// Create a global singleton instance
const globalForSSE = globalThis as unknown as {
  sseManager: SSEManager | undefined;
};

// If the instance exists, use it. Otherwise, create a new one.
export const sseManager = globalForSSE.sseManager ?? new SSEManager();

// Update the global reference
globalForSSE.sseManager = sseManager;
