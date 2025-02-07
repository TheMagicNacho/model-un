class Game {
  constructor() {
    this.server_state = {};
    this.local_state = {
      type: "None",
      player_id: 0,
      name: "",
      value: 0,
    };
  }
  async run() {
    const ws = await this.connect_to_server("ws://localhost:3000/ws");

    // Debaouncing is used for the field inputs to limit spamming the server.
    const debounce_time = 1;
    let debounce_timer;

    // Add event listener to name input
    const name_input = document.getElementById("player_name");
    name_input.addEventListener("input", () => {
      clearTimeout(debounce_timer);
      debounce_timer = setTimeout(() => {
        this.handle_name_change(this.local_state, ws);
      }, debounce_time);
    });
    name_input.focus();

    // Add event listener to value input
    const value_input = document.getElementById("player_value");
    value_input.addEventListener("change", () => {
      this.handle_value_change(this.local_state, ws);
    });

    // Add event listener to reveal button
    const reveal_button = document.getElementById("reveal-button");
    reveal_button.addEventListener("click", () => {
      this.handle_reveal_button_click(this.local_state, ws);
    });
  }

  handle_reveal_button_click(local_state, ws) {
    if (this.server_state.all_revealed) {
      // Set the Select to default value
      document.getElementById("player_value").value = 0;
    }

    const request = {};
    request.type = "RevealNumbers";
    request.value = !this.server_state.all_revealed;
    // console.log("Sending WebSocket message:", request);
    ws.send(JSON.stringify(request));
  }
  handle_value_change(local_state, ws) {
    local_state.value = parseInt(document.getElementById("player_value").value);
    const request = local_state;
    request.type = "ChangeValue";
    // console.log("Sending WebSocket message:", request);
    ws.send(JSON.stringify(request));
  }
  handle_name_change(local_state, ws) {
    local_state.name = document.getElementById("player_name").value;
    const request = local_state;
    request.type = "ChangeName";
    // console.log("Sending WebSocket message:", request);
    ws.send(JSON.stringify(request));
  }
  async connect_to_server(server_address) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(server_address);

      ws.onopen = () => {
        console.log("Connected to server");
        resolve(ws);
      };

      ws.onmessage = (event) => {
        const temp_state = JSON.parse(event.data);
        // console.log("Updated Server State:", temp_state);

        if (temp_state.type === "PlayerAssigned") {
          this.local_state.player_id = temp_state.player_id;
        }

        if (temp_state.type === "UpdateState") {
          this.server_state = temp_state;
          if (this.local_state.player_id === temp_state.notify_change.current_id){
            console.log("Previous Player ID:", this.local_state.player_id);
            this.local_state.player_id = temp_state.notify_change.new_id;
            console.log("Updated Player ID:", this.local_state.player_id);
          }
          this.update_dom_from_server_state();
          console.log("Updated Server State:", this.server_state);
        }

        if (temp_state.type === "Ping") {
          ws.send(
            JSON.stringify({
              type: "Pong",
              player_id: this.local_state.player_id,
            }),
          );
        }
      };

      ws.onerror = (error) => {
        console.error("WebSocket error:", error);
        reject(error);
      };
    });
  }
  update_dom_from_server_state() {
    // Clear all player cards to set them as vacant first
    for (let i = 0; i < 6; i++) {
      const player_card_id = "player" + i + "id";
      const player_card_element = document.getElementById(player_card_id);
      player_card_element.classList.remove("player-ready");

      if (player_card_element) {
        // Reset the name and value for all cards
        const player_name_element = document.getElementById(
          "player" + i + "name",
        );
        const player_value_element = document.getElementById(
          "player" + i + "value",
        );
        if (player_name_element) {
          player_name_element.innerHTML = `Player ${i}`;
        }
        if (player_value_element) {
          player_value_element.innerHTML = "?";
        }
      }
    }

    // Update the player cards based on server state
    for (const player of this.server_state.players) {
      if (player.player_id !== null) {
        const player_card_id = "player" + player.player_id + "id";
        const player_name_id = "player" + player.player_id + "name";
        const player_value_id = "player" + player.player_id + "value";

        // Get elements
        const player_card_element = document.getElementById(player_card_id);
        const player_name_element = document.getElementById(player_name_id);
        const player_value_element = document.getElementById(player_value_id);

        if (player_card_element) {
          // Mark the card as occupied
          player_card_element.classList.remove("player-vacant");
          if (player.value > 0) {
            player_card_element.classList.add("player-ready");
          }
        }
        if (player_name_element) {
          // Update the name
          player_name_element.innerHTML =
            player.player_name ?? `Player ${player.player_id}`;
        }
        if (player_value_element) {
          // Update the value only if revealed
          if (this.server_state.all_revealed) {
            player_value_element.innerHTML = player.value ? player.value : "?";
          } else {
            player_value_element.innerHTML = "?";
          }
        }
      }
    }

    // Update the reveal/reset button text
    const reveal_button_element = document.getElementById("reveal-button");
    if (reveal_button_element) {
      if (this.server_state.all_revealed) {
        reveal_button_element.innerHTML = "Reset";
      } else {
        reveal_button_element.innerHTML = "Reveal";
      }
    }
  }
}
const game = new Game();
game.run();
