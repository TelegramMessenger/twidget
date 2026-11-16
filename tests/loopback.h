// Both ends in one process: the platform end's input waits in a queue until pump(), and the renderer end's messages reach the platform end in order, the one-way ones before each request is answered.
#pragma once

#include "log_backend.h"
#include "platform_end.h"
#include "renderer_end.h"

#include <cstddef>
#include <cstdint>
#include <deque>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace {

struct Queue : remote::Channel {
    std::deque<std::string> messages;

    void send(std::string message) override { messages.push_back(std::move(message)); }
};

/// What the platform end gives its host, in order; a frame also marks the Backend's log.
struct Heard : remote::PlatformEnd::Host {
    explicit Heard(LogBackend& backend) : backend(backend) {}

    void frame(remote::Frame&& frame) override {
        events.push_back(remote::Kind::Frame);
        backend.frame(frame.painted);
        frames.push_back(std::move(frame));
    }
    void state(const remote::State& state) override {
        events.push_back(remote::Kind::State);
        states.push_back(state);
    }
    void messages(std::vector<twidget::Json>&& messages) override {
        events.push_back(remote::Kind::Messages);
        batches.push_back(std::move(messages));
    }
    void pointer(const remote::Pointer& pointer) override {
        events.push_back(remote::Kind::Pointer);
        pointers.push_back(pointer);
    }
    void selection(const remote::Selection& selection) override {
        events.push_back(remote::Kind::Selection);
        selections.push_back(selection);
    }
    void claim(const remote::ClaimAnswer& answer) override {
        events.push_back(remote::Kind::ClaimAnswer);
        claims.push_back(answer);
    }
    void refused(const std::string& reason) override { refusals.push_back(reason); }

    LogBackend& backend;
    std::vector<remote::Kind> events;
    std::vector<remote::Frame> frames;
    std::vector<remote::State> states;
    std::vector<std::vector<twidget::Json>> batches;
    std::vector<remote::Pointer> pointers;
    std::vector<remote::Selection> selections;
    std::vector<remote::ClaimAnswer> claims;
    std::vector<std::string> refusals;
};

/// What Wire does to the answer of the request a Fault names.
enum class Spoil { None, Unanswered, Empty, WrongKind, WrongVersion, Cut, Trailing, Replaced };

/// The request a Spoil hits: number `at` of the session, from 1, or else the next one of `kind`.
struct Fault {
    Spoil how = Spoil::None;
    size_t at = 0;
    std::optional<remote::Kind> kind;
    std::string reply;
};

/// The renderer end's channel, which calls the platform end itself.
struct Wire : remote::RendererChannel {
    explicit Wire(remote::PlatformEnd& platform) : platform(platform) {}

    void send(std::string message) override {
        kinds.push_back(kind(message));
        outbox.push_back(std::move(message));
    }

    bool request(std::string message, std::string& answer) override {
        const remote::Kind asked = kind(message);
        kinds.push_back(asked);
        requests++;
        flush();
        const bool hit = fault.how != Spoil::None && (fault.at ? fault.at == requests : fault.kind == asked);
        const Fault now = hit ? std::exchange(fault, Fault()) : Fault();
        if (hit) spoiled = kinds.size();
        if (now.how == Spoil::Unanswered) return false;
        std::string reply;
        if (!platform.answer(message, reply)) {
            refused++;
            return false;
        }
        spoil(now, reply);
        answer = std::move(reply);
        return true;
    }

    /// Hands the platform end what the renderer end sent, in order.
    void flush() {
        while (!outbox.empty()) {
            const std::string message = std::move(outbox.front());
            outbox.pop_front();
            if (!platform.receive(message)) refused++;
        }
    }

    static remote::Kind kind(const std::string& message) { return remote::Kind(message.empty() ? 0 : uint8_t(message[0])); }

    static void spoil(const Fault& fault, std::string& reply) {
        switch (fault.how) {
        case Spoil::Empty: reply.clear(); break;
        case Spoil::WrongKind: reply[0] = char(uint8_t(remote::Kind::FontAnswer) + (uint8_t(reply[0]) - uint8_t(remote::Kind::FontAnswer) + 1) % 4); break;
        case Spoil::WrongVersion: reply[1] = char(remote::VERSION + 1); break;
        case Spoil::Cut: reply.pop_back(); break;
        case Spoil::Trailing: reply.push_back('\0'); break;
        case Spoil::Replaced: reply = fault.reply; break;
        default: break;
        }
    }

    remote::PlatformEnd& platform;
    std::deque<std::string> outbox;
    /// Every message of the renderer end, requests included, by kind.
    std::vector<remote::Kind> kinds;
    size_t requests = 0;
    /// Messages and requests the platform end refused.
    size_t refused = 0;
    Fault fault;
    /// How many messages the renderer end had sent with the spoiled request, 0 before one.
    size_t spoiled = 0;
};

/// The backend, the host, the queue, the platform end, the channel and then the renderer end, made and dropped in that order.
struct Loopback {
    explicit Loopback(const remote::Caps& caps = {}) : caps(caps), platform(backend, host, queue, caps), wire(platform) {}

    void open(double width, double dpr, bool dark, Clock* clock = nullptr) {
        end = std::make_unique<remote::RendererEnd>(width, dpr, dark, wire, caps, clock);
        wire.flush();
    }

    /// Drops the renderer end while everything else lives, and hands on what it sent going.
    void close() {
        end.reset();
        wire.flush();
    }

    std::vector<twidget::Json> receive(const twidget::Json& message) {
        std::vector<twidget::Json> messages = end->receive(message);
        wire.flush();
        return messages;
    }

    void want_frame() {
        end->want_frame();
        wire.flush();
    }

    /// Delivers the queued platform input one message at a time into the same Output, each call's traffic passed on after it returns; false past `steps` messages.
    bool pump(size_t steps = 64) {
        wire.flush();
        for (size_t i = 0; !queue.messages.empty(); i++) {
            if (i == steps || !end) return false;
            const std::string message = std::move(queue.messages.front());
            queue.messages.pop_front();
            delivered.push_back(end->deliver(message, out));
            outputs.push_back(out);
            wire.flush();
        }
        return true;
    }

    remote::Caps caps;
    LogBackend backend;
    Heard host{backend};
    Queue queue;
    remote::PlatformEnd platform;
    Wire wire;
    std::unique_ptr<remote::RendererEnd> end;
    remote::RendererEnd::Output out;
    std::vector<bool> delivered;
    std::vector<remote::RendererEnd::Output> outputs;
};

} // namespace
