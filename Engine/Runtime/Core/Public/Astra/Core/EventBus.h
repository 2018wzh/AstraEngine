#pragma once

#include <algorithm>
#include <functional>
#include <typeindex>
#include <unordered_map>
#include <vector>

namespace astra {

struct EventSubscription {
    std::type_index type{typeid(void)};
    std::size_t id = 0;
};

class EventBus {
  public:
    template <typename Event>
    EventSubscription subscribe(std::function<void(const Event&)> callback) {
        const std::type_index type = std::type_index(typeid(Event));
        const std::size_t id = next_id_++;
        auto wrapper = [callback = std::move(callback)](const void* event) {
            callback(*static_cast<const Event*>(event));
        };
        handlers_[type].push_back(Handler{id, std::move(wrapper)});
        return EventSubscription{type, id};
    }

    void unsubscribe(EventSubscription subscription) {
        auto it = handlers_.find(subscription.type);
        if (it == handlers_.end()) {
            return;
        }
        auto& list = it->second;
        std::erase_if(list, [subscription](const Handler& handler) {
            return handler.id == subscription.id;
        });
    }

    template <typename Event>
    void publish(const Event& event) const {
        const std::type_index type = std::type_index(typeid(Event));
        const auto it = handlers_.find(type);
        if (it == handlers_.end()) {
            return;
        }
        for (const Handler& handler : it->second) {
            handler.callback(&event);
        }
    }

    void clear() {
        handlers_.clear();
    }

  private:
    struct Handler {
        std::size_t id = 0;
        std::function<void(const void*)> callback;
    };

    std::unordered_map<std::type_index, std::vector<Handler>> handlers_;
    std::size_t next_id_ = 1;
};

} // namespace astra
