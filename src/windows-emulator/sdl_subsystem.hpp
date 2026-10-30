#pragma once

#include <SDL3/SDL.h>

#include <mutex>
#include <thread>

// SDL keeps its subsystem reference counts and video state in unsynchronized globals, and only the thread that
// initialized video may use it. Every emulator owns its own SDL backends on its own host thread, so all of them
// initialize and quit subsystems through here, and video is refused to other threads while one thread holds it.
namespace sogen::sdl
{
    namespace detail
    {
        struct subsystem_state
        {
            std::mutex mutex{};
            std::thread::id video_thread{};
            size_t video_references{};
        };

        inline subsystem_state& get_subsystem_state()
        {
            static subsystem_state state{};
            return state;
        }
    }

    inline bool init_subsystem(const SDL_InitFlags flags)
    {
        auto& state = detail::get_subsystem_state();
        const std::lock_guard lock{state.mutex};

        const auto video = (flags & SDL_INIT_VIDEO) != 0;
        if (video && state.video_references != 0 && state.video_thread != std::this_thread::get_id())
        {
            return SDL_SetError("SDL video is in use on another thread");
        }

        if (!SDL_InitSubSystem(flags))
        {
            return false;
        }

        if (video)
        {
            state.video_thread = std::this_thread::get_id();
            ++state.video_references;
        }

        return true;
    }

    inline void quit_subsystem(const SDL_InitFlags flags)
    {
        auto& state = detail::get_subsystem_state();
        const std::lock_guard lock{state.mutex};

        SDL_QuitSubSystem(flags);

        if ((flags & SDL_INIT_VIDEO) != 0)
        {
            --state.video_references;
        }
    }
}
