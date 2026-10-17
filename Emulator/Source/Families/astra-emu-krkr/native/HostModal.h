#pragma once
#include "HostModalAbi.h"
#include "NativeOwner.h"
#include "environ/EmbeddedModal.h"
#include <array>
#include <chrono>
#include <thread>
#include <memory>
#include <stdexcept>

namespace astra::krkr {
class HostModal final {
    void *context_;
    AstraKrkrModalCallback callback_;
    TVPEmbeddedModalHost host_;
    const std::thread::id owner_ = std::this_thread::get_id();
    std::chrono::steady_clock::duration waiting_{};
    struct WaitTime {
        HostModal &host;
        const std::chrono::steady_clock::time_point start = std::chrono::steady_clock::now();
        ~WaitTime() {
            if(std::this_thread::get_id() == host.owner_)
                host.waiting_ += std::chrono::steady_clock::now() - start;
        }
    };

    static AstraKrkrModalText span(const std::string &text) noexcept {
        return {reinterpret_cast<const std::uint8_t *>(text.data()), text.size()};
    }
    static int request(void *context, const std::string &caption,
                       const std::string &message,
                       const std::vector<std::string> &buttons,
                       std::string *input) {
        auto &self = *static_cast<HostModal *>(context);
        if(caption.size() > 65536 || message.size() > 65536 ||
           buttons.empty() || buttons.size() > 16 ||
           (input && input->size() > 65536))
            throw std::runtime_error("ASTRA_KRKR_MODAL_REQUEST_BOUNDS");
        std::array<AstraKrkrModalText, 16> labels;
        for(std::size_t index = 0; index < buttons.size(); ++index) {
            if(buttons[index].size() > 256)
                throw std::runtime_error("ASTRA_KRKR_MODAL_REQUEST_BOUNDS");
            labels[index] = span(buttons[index]);
        }
        const AstraKrkrModalRequest request{
            span(caption), span(message), labels.data(),
            static_cast<std::uint32_t>(buttons.size()), input ? 1u : 0u,
            input ? span(*input) : AstraKrkrModalText{nullptr, 0}};
        std::unique_ptr<std::uint8_t[]> text;
        if(input)
            text.reset(new std::uint8_t[65536]);
        AstraKrkrModalResponse response{0, 0, text.get(), input ? 65536u : 0u, 0};
        const WaitTime wait{self};
        const auto status = self.callback_(self.context_, &request, &response);
        // NativeOwner retains the first typed host failure for this operation;
        // an error-reporting modal must not replace the preceding window error.
        if(status == 5) throw NativeCancelled();
        if(status != 0) throw std::runtime_error("ASTRA_KRKR_HOST_MODAL_FAILED");
        if(response.kind == 0)
            return -1;
        if(response.kind != 1 || response.button_index >= buttons.size() ||
           response.text_bytes > response.text_capacity ||
           response.text != text.get() ||
           response.text_capacity != (input ? 65536u : 0u))
            throw std::runtime_error("ASTRA_KRKR_MODAL_RESPONSE_BOUNDS");
        if(input)
            input->assign(reinterpret_cast<const char *>(text.get()), response.text_bytes);
        else if(response.text_bytes != 0)
            throw std::runtime_error("ASTRA_KRKR_MODAL_RESPONSE_KIND");
        return static_cast<int>(response.button_index);
    }

public:
    std::chrono::steady_clock::duration owner_waiting() const noexcept { return waiting_; }
    HostModal(void *context, AstraKrkrModalCallback callback)
        : context_(context), callback_(callback), host_{this, request} {
        if(!context_ || !callback_ || !TVPInstallEmbeddedModalHost(&host_))
            throw std::runtime_error("ASTRA_KRKR_HOST_MODAL_INSTALL");
    }
    HostModal(const HostModal &) = delete;
    HostModal &operator=(const HostModal &) = delete;
    HostModal(HostModal &&) = delete;
    HostModal &operator=(HostModal &&) = delete;
    ~HostModal() { TVPRemoveEmbeddedModalHost(&host_); }
};
}
