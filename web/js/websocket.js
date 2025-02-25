/**
 * WebSocket Manager for EHR System
 * Handles real-time communication with the server for vitals monitoring
 */

class WebSocketManager {
    constructor(url) {
        this.url = url;
        this.socket = null;
        this.reconnectAttempts = 0;
        this.maxReconnectAttempts = 5;
        this.reconnectDelay = 2000; // Start with 2 seconds
        this.callbacks = {
            onOpen: [],
            onMessage: [],
            onClose: [],
            onError: [],
            onReconnect: []
        };
        
        this.connect();
    }
    
    connect() {
        try {
            this.socket = new WebSocket(this.url);
            
            this.socket.onopen = (event) => {
                console.log('WebSocket connection established');
                this.reconnectAttempts = 0;
                this.reconnectDelay = 2000;
                this._triggerCallbacks('onOpen', event);
            };
            
            this.socket.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    this._triggerCallbacks('onMessage', data);
                } catch (error) {
                    console.error('Error parsing WebSocket message:', error);
                }
            };
            
            this.socket.onclose = (event) => {
                console.log('WebSocket connection closed');
                this._triggerCallbacks('onClose', event);
                
                if (this.reconnectAttempts < this.maxReconnectAttempts) {
                    this._attemptReconnect();
                } else {
                    console.error('Maximum reconnection attempts reached');
                }
            };
            
            this.socket.onerror = (error) => {
                console.error('WebSocket error:', error);
                this._triggerCallbacks('onError', error);
            };
        } catch (error) {
            console.error('Error creating WebSocket connection:', error);
            this._attemptReconnect();
        }
    }
    
    _attemptReconnect() {
        this.reconnectAttempts++;
        console.log(`Attempting to reconnect (${this.reconnectAttempts}/${this.maxReconnectAttempts})...`);
        
        this._triggerCallbacks('onReconnect', {
            attempt: this.reconnectAttempts,
            maxAttempts: this.maxReconnectAttempts
        });
        
        setTimeout(() => {
            this.connect();
        }, this.reconnectDelay);
        
        // Exponential backoff for reconnect attempts
        this.reconnectDelay = Math.min(this.reconnectDelay * 1.5, 30000); // Max 30 seconds
    }
    
    send(data) {
        if (this.socket && this.socket.readyState === WebSocket.OPEN) {
            if (typeof data === 'object') {
                this.socket.send(JSON.stringify(data));
            } else {
                this.socket.send(data);
            }
            return true;
        } else {
            console.error('Cannot send message: WebSocket is not connected');
            return false;
        }
    }
    
    on(event, callback) {
        if (this.callbacks[event]) {
            this.callbacks[event].push(callback);
        } else {
            console.error(`Unknown event type: ${event}`);
        }
    }
    
    off(event, callback) {
        if (this.callbacks[event]) {
            this.callbacks[event] = this.callbacks[event].filter(cb => cb !== callback);
        }
    }
    
    _triggerCallbacks(event, data) {
        if (this.callbacks[event]) {
            this.callbacks[event].forEach(callback => {
                try {
                    callback(data);
                } catch (error) {
                    console.error(`Error in ${event} callback:`, error);
                }
            });
        }
    }
    
    close() {
        if (this.socket) {
            this.socket.close();
        }
    }
    
    getState() {
        if (!this.socket) return 'CLOSED';
        
        switch (this.socket.readyState) {
            case WebSocket.CONNECTING:
                return 'CONNECTING';
            case WebSocket.OPEN:
                return 'OPEN';
            case WebSocket.CLOSING:
                return 'CLOSING';
            case WebSocket.CLOSED:
                return 'CLOSED';
            default:
                return 'UNKNOWN';
        }
    }
}

// Export for use in other modules
window.WebSocketManager = WebSocketManager; 