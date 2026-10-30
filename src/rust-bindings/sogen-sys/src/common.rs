#[cxx::bridge(namespace = "sogen::ffi")]
pub mod ffi {
    #[namespace = "sogen"]
    #[cxx_name = "backend_type"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum BackendType {
        #[cxx_name = "unicorn"]
        Unicorn = 0,
        #[cxx_name = "icicle"]
        Icicle = 1,
        #[cxx_name = "whp"]
        Whp = 2,
        #[cxx_name = "kvm"]
        Kvm = 3,
        #[cxx_name = "fex"]
        Fex = 4,
        #[cxx_name = "hvf"]
        Hvf = 5,
    }

    #[namespace = "sogen"]
    #[cxx_name = "memory_permission"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum MemoryPermission {
        #[cxx_name = "none"]
        None = 0,
        #[cxx_name = "read"]
        Read = 1,
        #[cxx_name = "write"]
        Write = 2,
        #[cxx_name = "exec"]
        Exec = 4,
        #[cxx_name = "read_write"]
        ReadWrite = 3,
        #[cxx_name = "read_exec"]
        ReadExec = 5,
        #[cxx_name = "write_exec"]
        WriteExec = 6,
        #[cxx_name = "all"]
        All = 7,
        #[cxx_name = "guard"]
        Guard = 8,
    }

    #[namespace = "sogen"]
    #[cxx_name = "stop_reason"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum StopReason {
        #[cxx_name = "none"]
        None = 0,
        #[cxx_name = "unknown_syscall"]
        UnknownSyscall = 1,
        #[cxx_name = "unimplemented_syscall"]
        UnimplementedSyscall = 2,
        #[cxx_name = "syscall_exception"]
        SyscallException = 3,
        #[cxx_name = "instruction_limit"]
        InstructionLimit = 4,
        #[cxx_name = "normal_exit"]
        NormalExit = 5,
        #[cxx_name = "signal_termination"]
        SignalTermination = 6,
        #[cxx_name = "unhandled_memory_violation"]
        UnhandledMemoryViolation = 7,
        #[cxx_name = "explicit_stop"]
        ExplicitStop = 8,
        #[cxx_name = "backend_error"]
        BackendError = 9,
        #[cxx_name = "breakpoint"]
        Breakpoint = 10,
        #[cxx_name = "watchpoint"]
        Watchpoint = 11,
    }

    #[namespace = "sogen"]
    #[cxx_name = "instruction_hook_continuation"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum InstructionHookContinuation {
        #[cxx_name = "run_instruction"]
        RunInstruction = 0,
        #[cxx_name = "skip_instruction"]
        SkipInstruction = 1,
        #[cxx_name = "finalized_instruction_pointer"]
        FinalizedInstructionPointer = 2,
    }

    #[namespace = "sogen"]
    #[cxx_name = "memory_violation_continuation"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum MemoryViolationContinuation {
        #[cxx_name = "stop"]
        Stop = 0,
        #[cxx_name = "resume"]
        Resume = 1,
        #[cxx_name = "restart"]
        Restart = 2,
    }

    #[namespace = "sogen"]
    #[cxx_name = "memory_violation_type"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum MemoryViolationType {
        #[cxx_name = "unmapped"]
        Unmapped = 0,
        #[cxx_name = "protection"]
        Protection = 1,
    }

    #[namespace = "sogen"]
    #[cxx_name = "memory_region_kind"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum MemoryRegionKind {
        #[cxx_name = "free"]
        Free = 0,
        #[cxx_name = "private_allocation"]
        PrivateAllocation = 1,
        #[cxx_name = "file_section_view"]
        FileSectionView = 2,
        #[cxx_name = "pagefile_section_view"]
        PagefileSectionView = 3,
        #[cxx_name = "section_image"]
        SectionImage = 4,
        #[cxx_name = "mmio"]
        Mmio = 5,
        #[cxx_name = "host_reserved"]
        HostReserved = 6,
        #[cxx_name = "placeholder"]
        Placeholder = 7,
        #[cxx_name = "placeholder_backed_allocation"]
        PlaceholderBackedAllocation = 8,
    }

    #[namespace = "sogen"]
    #[cxx_name = "color"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum LogLevel {
        #[cxx_name = "red"]
        Error = 1,
        #[cxx_name = "green"]
        Success = 2,
        #[cxx_name = "yellow"]
        Warn = 3,
        #[cxx_name = "cyan"]
        Info = 5,
        #[cxx_name = "gray"]
        Log = 8,
    }

    #[cxx_name = "memory_execution_hook_mode"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum MemoryExecutionHookMode {
        #[cxx_name = "automatic"]
        Automatic = 0,
        #[cxx_name = "int3"]
        Int3 = 1,
    }

    #[namespace = "sogen"]
    #[cxx_name = "x86_hookable_instructions"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum X86HookableInstruction {
        #[cxx_name = "invalid"]
        Invalid = 0,
        #[cxx_name = "syscall"]
        Syscall = 1,
        #[cxx_name = "cpuid"]
        Cpuid = 2,
        #[cxx_name = "rdtsc"]
        Rdtsc = 3,
        #[cxx_name = "rdtscp"]
        Rdtscp = 4,
    }

    #[namespace = "sogen"]
    #[cxx_name = "arm64_hookable_instructions"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum Arm64HookableInstruction {
        #[cxx_name = "svc"]
        Svc = 0,
        #[cxx_name = "read_counter"]
        ReadCounter = 1,
    }

    #[namespace = "sogen"]
    #[cxx_name = "x86_register"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum X86Register {
        #[cxx_name = "invalid"]
        Invalid = 0,
        #[cxx_name = "ah"]
        Ah = 1,
        #[cxx_name = "al"]
        Al = 2,
        #[cxx_name = "ax"]
        Ax = 3,
        #[cxx_name = "bh"]
        Bh = 4,
        #[cxx_name = "bl"]
        Bl = 5,
        #[cxx_name = "bp"]
        Bp = 6,
        #[cxx_name = "bpl"]
        Bpl = 7,
        #[cxx_name = "bx"]
        Bx = 8,
        #[cxx_name = "ch"]
        Ch = 9,
        #[cxx_name = "cl"]
        Cl = 10,
        #[cxx_name = "cs"]
        Cs = 11,
        #[cxx_name = "cx"]
        Cx = 12,
        #[cxx_name = "dh"]
        Dh = 13,
        #[cxx_name = "di"]
        Di = 14,
        #[cxx_name = "dil"]
        Dil = 15,
        #[cxx_name = "dl"]
        Dl = 16,
        #[cxx_name = "ds"]
        Ds = 17,
        #[cxx_name = "dx"]
        Dx = 18,
        #[cxx_name = "eax"]
        Eax = 19,
        #[cxx_name = "ebp"]
        Ebp = 20,
        #[cxx_name = "ebx"]
        Ebx = 21,
        #[cxx_name = "ecx"]
        Ecx = 22,
        #[cxx_name = "edi"]
        Edi = 23,
        #[cxx_name = "edx"]
        Edx = 24,
        #[cxx_name = "eflags"]
        Eflags = 25,
        #[cxx_name = "eip"]
        Eip = 26,
        #[cxx_name = "es"]
        Es = 28,
        #[cxx_name = "esi"]
        Esi = 29,
        #[cxx_name = "esp"]
        Esp = 30,
        #[cxx_name = "fpsw"]
        Fpsw = 31,
        #[cxx_name = "fs"]
        Fs = 32,
        #[cxx_name = "gs"]
        Gs = 33,
        #[cxx_name = "ip"]
        Ip = 34,
        #[cxx_name = "rax"]
        Rax = 35,
        #[cxx_name = "rbp"]
        Rbp = 36,
        #[cxx_name = "rbx"]
        Rbx = 37,
        #[cxx_name = "rcx"]
        Rcx = 38,
        #[cxx_name = "rdi"]
        Rdi = 39,
        #[cxx_name = "rdx"]
        Rdx = 40,
        #[cxx_name = "rip"]
        Rip = 41,
        #[cxx_name = "rsi"]
        Rsi = 43,
        #[cxx_name = "rsp"]
        Rsp = 44,
        #[cxx_name = "si"]
        Si = 45,
        #[cxx_name = "sil"]
        Sil = 46,
        #[cxx_name = "sp"]
        Sp = 47,
        #[cxx_name = "spl"]
        Spl = 48,
        #[cxx_name = "ss"]
        Ss = 49,
        #[cxx_name = "cr0"]
        Cr0 = 50,
        #[cxx_name = "cr1"]
        Cr1 = 51,
        #[cxx_name = "cr2"]
        Cr2 = 52,
        #[cxx_name = "cr3"]
        Cr3 = 53,
        #[cxx_name = "cr4"]
        Cr4 = 54,
        #[cxx_name = "cr8"]
        Cr8 = 58,
        #[cxx_name = "dr0"]
        Dr0 = 66,
        #[cxx_name = "dr1"]
        Dr1 = 67,
        #[cxx_name = "dr2"]
        Dr2 = 68,
        #[cxx_name = "dr3"]
        Dr3 = 69,
        #[cxx_name = "dr4"]
        Dr4 = 70,
        #[cxx_name = "dr5"]
        Dr5 = 71,
        #[cxx_name = "dr6"]
        Dr6 = 72,
        #[cxx_name = "dr7"]
        Dr7 = 73,
        #[cxx_name = "fp0"]
        Fp0 = 82,
        #[cxx_name = "fp1"]
        Fp1 = 83,
        #[cxx_name = "fp2"]
        Fp2 = 84,
        #[cxx_name = "fp3"]
        Fp3 = 85,
        #[cxx_name = "fp4"]
        Fp4 = 86,
        #[cxx_name = "fp5"]
        Fp5 = 87,
        #[cxx_name = "fp6"]
        Fp6 = 88,
        #[cxx_name = "fp7"]
        Fp7 = 89,
        #[cxx_name = "k0"]
        K0 = 90,
        #[cxx_name = "k1"]
        K1 = 91,
        #[cxx_name = "k2"]
        K2 = 92,
        #[cxx_name = "k3"]
        K3 = 93,
        #[cxx_name = "k4"]
        K4 = 94,
        #[cxx_name = "k5"]
        K5 = 95,
        #[cxx_name = "k6"]
        K6 = 96,
        #[cxx_name = "k7"]
        K7 = 97,
        #[cxx_name = "mm0"]
        Mm0 = 98,
        #[cxx_name = "mm1"]
        Mm1 = 99,
        #[cxx_name = "mm2"]
        Mm2 = 100,
        #[cxx_name = "mm3"]
        Mm3 = 101,
        #[cxx_name = "mm4"]
        Mm4 = 102,
        #[cxx_name = "mm5"]
        Mm5 = 103,
        #[cxx_name = "mm6"]
        Mm6 = 104,
        #[cxx_name = "mm7"]
        Mm7 = 105,
        #[cxx_name = "r8"]
        R8 = 106,
        #[cxx_name = "r9"]
        R9 = 107,
        #[cxx_name = "r10"]
        R10 = 108,
        #[cxx_name = "r11"]
        R11 = 109,
        #[cxx_name = "r12"]
        R12 = 110,
        #[cxx_name = "r13"]
        R13 = 111,
        #[cxx_name = "r14"]
        R14 = 112,
        #[cxx_name = "r15"]
        R15 = 113,
        #[cxx_name = "st0"]
        St0 = 114,
        #[cxx_name = "st1"]
        St1 = 115,
        #[cxx_name = "st2"]
        St2 = 116,
        #[cxx_name = "st3"]
        St3 = 117,
        #[cxx_name = "st4"]
        St4 = 118,
        #[cxx_name = "st5"]
        St5 = 119,
        #[cxx_name = "st6"]
        St6 = 120,
        #[cxx_name = "st7"]
        St7 = 121,
        #[cxx_name = "xmm0"]
        Xmm0 = 122,
        #[cxx_name = "xmm1"]
        Xmm1 = 123,
        #[cxx_name = "xmm2"]
        Xmm2 = 124,
        #[cxx_name = "xmm3"]
        Xmm3 = 125,
        #[cxx_name = "xmm4"]
        Xmm4 = 126,
        #[cxx_name = "xmm5"]
        Xmm5 = 127,
        #[cxx_name = "xmm6"]
        Xmm6 = 128,
        #[cxx_name = "xmm7"]
        Xmm7 = 129,
        #[cxx_name = "xmm8"]
        Xmm8 = 130,
        #[cxx_name = "xmm9"]
        Xmm9 = 131,
        #[cxx_name = "xmm10"]
        Xmm10 = 132,
        #[cxx_name = "xmm11"]
        Xmm11 = 133,
        #[cxx_name = "xmm12"]
        Xmm12 = 134,
        #[cxx_name = "xmm13"]
        Xmm13 = 135,
        #[cxx_name = "xmm14"]
        Xmm14 = 136,
        #[cxx_name = "xmm15"]
        Xmm15 = 137,
        #[cxx_name = "xmm16"]
        Xmm16 = 138,
        #[cxx_name = "xmm17"]
        Xmm17 = 139,
        #[cxx_name = "xmm18"]
        Xmm18 = 140,
        #[cxx_name = "xmm19"]
        Xmm19 = 141,
        #[cxx_name = "xmm20"]
        Xmm20 = 142,
        #[cxx_name = "xmm21"]
        Xmm21 = 143,
        #[cxx_name = "xmm22"]
        Xmm22 = 144,
        #[cxx_name = "xmm23"]
        Xmm23 = 145,
        #[cxx_name = "xmm24"]
        Xmm24 = 146,
        #[cxx_name = "xmm25"]
        Xmm25 = 147,
        #[cxx_name = "xmm26"]
        Xmm26 = 148,
        #[cxx_name = "xmm27"]
        Xmm27 = 149,
        #[cxx_name = "xmm28"]
        Xmm28 = 150,
        #[cxx_name = "xmm29"]
        Xmm29 = 151,
        #[cxx_name = "xmm30"]
        Xmm30 = 152,
        #[cxx_name = "xmm31"]
        Xmm31 = 153,
        #[cxx_name = "ymm0"]
        Ymm0 = 154,
        #[cxx_name = "ymm1"]
        Ymm1 = 155,
        #[cxx_name = "ymm2"]
        Ymm2 = 156,
        #[cxx_name = "ymm3"]
        Ymm3 = 157,
        #[cxx_name = "ymm4"]
        Ymm4 = 158,
        #[cxx_name = "ymm5"]
        Ymm5 = 159,
        #[cxx_name = "ymm6"]
        Ymm6 = 160,
        #[cxx_name = "ymm7"]
        Ymm7 = 161,
        #[cxx_name = "ymm8"]
        Ymm8 = 162,
        #[cxx_name = "ymm9"]
        Ymm9 = 163,
        #[cxx_name = "ymm10"]
        Ymm10 = 164,
        #[cxx_name = "ymm11"]
        Ymm11 = 165,
        #[cxx_name = "ymm12"]
        Ymm12 = 166,
        #[cxx_name = "ymm13"]
        Ymm13 = 167,
        #[cxx_name = "ymm14"]
        Ymm14 = 168,
        #[cxx_name = "ymm15"]
        Ymm15 = 169,
        #[cxx_name = "ymm16"]
        Ymm16 = 170,
        #[cxx_name = "ymm17"]
        Ymm17 = 171,
        #[cxx_name = "ymm18"]
        Ymm18 = 172,
        #[cxx_name = "ymm19"]
        Ymm19 = 173,
        #[cxx_name = "ymm20"]
        Ymm20 = 174,
        #[cxx_name = "ymm21"]
        Ymm21 = 175,
        #[cxx_name = "ymm22"]
        Ymm22 = 176,
        #[cxx_name = "ymm23"]
        Ymm23 = 177,
        #[cxx_name = "ymm24"]
        Ymm24 = 178,
        #[cxx_name = "ymm25"]
        Ymm25 = 179,
        #[cxx_name = "ymm26"]
        Ymm26 = 180,
        #[cxx_name = "ymm27"]
        Ymm27 = 181,
        #[cxx_name = "ymm28"]
        Ymm28 = 182,
        #[cxx_name = "ymm29"]
        Ymm29 = 183,
        #[cxx_name = "ymm30"]
        Ymm30 = 184,
        #[cxx_name = "ymm31"]
        Ymm31 = 185,
        #[cxx_name = "zmm0"]
        Zmm0 = 186,
        #[cxx_name = "zmm1"]
        Zmm1 = 187,
        #[cxx_name = "zmm2"]
        Zmm2 = 188,
        #[cxx_name = "zmm3"]
        Zmm3 = 189,
        #[cxx_name = "zmm4"]
        Zmm4 = 190,
        #[cxx_name = "zmm5"]
        Zmm5 = 191,
        #[cxx_name = "zmm6"]
        Zmm6 = 192,
        #[cxx_name = "zmm7"]
        Zmm7 = 193,
        #[cxx_name = "zmm8"]
        Zmm8 = 194,
        #[cxx_name = "zmm9"]
        Zmm9 = 195,
        #[cxx_name = "zmm10"]
        Zmm10 = 196,
        #[cxx_name = "zmm11"]
        Zmm11 = 197,
        #[cxx_name = "zmm12"]
        Zmm12 = 198,
        #[cxx_name = "zmm13"]
        Zmm13 = 199,
        #[cxx_name = "zmm14"]
        Zmm14 = 200,
        #[cxx_name = "zmm15"]
        Zmm15 = 201,
        #[cxx_name = "zmm16"]
        Zmm16 = 202,
        #[cxx_name = "zmm17"]
        Zmm17 = 203,
        #[cxx_name = "zmm18"]
        Zmm18 = 204,
        #[cxx_name = "zmm19"]
        Zmm19 = 205,
        #[cxx_name = "zmm20"]
        Zmm20 = 206,
        #[cxx_name = "zmm21"]
        Zmm21 = 207,
        #[cxx_name = "zmm22"]
        Zmm22 = 208,
        #[cxx_name = "zmm23"]
        Zmm23 = 209,
        #[cxx_name = "zmm24"]
        Zmm24 = 210,
        #[cxx_name = "zmm25"]
        Zmm25 = 211,
        #[cxx_name = "zmm26"]
        Zmm26 = 212,
        #[cxx_name = "zmm27"]
        Zmm27 = 213,
        #[cxx_name = "zmm28"]
        Zmm28 = 214,
        #[cxx_name = "zmm29"]
        Zmm29 = 215,
        #[cxx_name = "zmm30"]
        Zmm30 = 216,
        #[cxx_name = "zmm31"]
        Zmm31 = 217,
        #[cxx_name = "r8b"]
        R8b = 218,
        #[cxx_name = "r9b"]
        R9b = 219,
        #[cxx_name = "r10b"]
        R10b = 220,
        #[cxx_name = "r11b"]
        R11b = 221,
        #[cxx_name = "r12b"]
        R12b = 222,
        #[cxx_name = "r13b"]
        R13b = 223,
        #[cxx_name = "r14b"]
        R14b = 224,
        #[cxx_name = "r15b"]
        R15b = 225,
        #[cxx_name = "r8d"]
        R8d = 226,
        #[cxx_name = "r9d"]
        R9d = 227,
        #[cxx_name = "r10d"]
        R10d = 228,
        #[cxx_name = "r11d"]
        R11d = 229,
        #[cxx_name = "r12d"]
        R12d = 230,
        #[cxx_name = "r13d"]
        R13d = 231,
        #[cxx_name = "r14d"]
        R14d = 232,
        #[cxx_name = "r15d"]
        R15d = 233,
        #[cxx_name = "r8w"]
        R8w = 234,
        #[cxx_name = "r9w"]
        R9w = 235,
        #[cxx_name = "r10w"]
        R10w = 236,
        #[cxx_name = "r11w"]
        R11w = 237,
        #[cxx_name = "r12w"]
        R12w = 238,
        #[cxx_name = "r13w"]
        R13w = 239,
        #[cxx_name = "r14w"]
        R14w = 240,
        #[cxx_name = "r15w"]
        R15w = 241,
        #[cxx_name = "idtr"]
        Idtr = 242,
        #[cxx_name = "gdtr"]
        Gdtr = 243,
        #[cxx_name = "ldtr"]
        Ldtr = 244,
        #[cxx_name = "tr"]
        Tr = 245,
        #[cxx_name = "fpcw"]
        Fpcw = 246,
        #[cxx_name = "fptag"]
        Fptag = 247,
        #[cxx_name = "msr"]
        Msr = 248,
        #[cxx_name = "mxcsr"]
        Mxcsr = 249,
        #[cxx_name = "fs_base"]
        FsBase = 250,
        #[cxx_name = "gs_base"]
        GsBase = 251,
        #[cxx_name = "flags"]
        Flags = 252,
        #[cxx_name = "rflags"]
        Rflags = 253,
        #[cxx_name = "fip"]
        Fip = 254,
        #[cxx_name = "fcs"]
        Fcs = 255,
        #[cxx_name = "fdp"]
        Fdp = 256,
        #[cxx_name = "fds"]
        Fds = 257,
        #[cxx_name = "fop"]
        Fop = 258,
        #[cxx_name = "xcr0"]
        Xcr0 = 259,
    }

    #[namespace = "sogen"]
    #[cxx_name = "arm64_register"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum Arm64Register {
        #[cxx_name = "invalid"]
        Invalid = 0,
        #[cxx_name = "x29"]
        X29 = 1,
        #[cxx_name = "x30"]
        X30 = 2,
        #[cxx_name = "nzcv"]
        Nzcv = 3,
        #[cxx_name = "sp"]
        Sp = 4,
        #[cxx_name = "wsp"]
        Wsp = 5,
        #[cxx_name = "wzr"]
        Wzr = 6,
        #[cxx_name = "xzr"]
        Xzr = 7,
        #[cxx_name = "q0"]
        Q0 = 104,
        #[cxx_name = "q1"]
        Q1 = 105,
        #[cxx_name = "q2"]
        Q2 = 106,
        #[cxx_name = "q3"]
        Q3 = 107,
        #[cxx_name = "q4"]
        Q4 = 108,
        #[cxx_name = "q5"]
        Q5 = 109,
        #[cxx_name = "q6"]
        Q6 = 110,
        #[cxx_name = "q7"]
        Q7 = 111,
        #[cxx_name = "q8"]
        Q8 = 112,
        #[cxx_name = "q9"]
        Q9 = 113,
        #[cxx_name = "q10"]
        Q10 = 114,
        #[cxx_name = "q11"]
        Q11 = 115,
        #[cxx_name = "q12"]
        Q12 = 116,
        #[cxx_name = "q13"]
        Q13 = 117,
        #[cxx_name = "q14"]
        Q14 = 118,
        #[cxx_name = "q15"]
        Q15 = 119,
        #[cxx_name = "q16"]
        Q16 = 120,
        #[cxx_name = "q17"]
        Q17 = 121,
        #[cxx_name = "q18"]
        Q18 = 122,
        #[cxx_name = "q19"]
        Q19 = 123,
        #[cxx_name = "q20"]
        Q20 = 124,
        #[cxx_name = "q21"]
        Q21 = 125,
        #[cxx_name = "q22"]
        Q22 = 126,
        #[cxx_name = "q23"]
        Q23 = 127,
        #[cxx_name = "q24"]
        Q24 = 128,
        #[cxx_name = "q25"]
        Q25 = 129,
        #[cxx_name = "q26"]
        Q26 = 130,
        #[cxx_name = "q27"]
        Q27 = 131,
        #[cxx_name = "q28"]
        Q28 = 132,
        #[cxx_name = "q29"]
        Q29 = 133,
        #[cxx_name = "q30"]
        Q30 = 134,
        #[cxx_name = "q31"]
        Q31 = 135,
        #[cxx_name = "w0"]
        W0 = 168,
        #[cxx_name = "w1"]
        W1 = 169,
        #[cxx_name = "w2"]
        W2 = 170,
        #[cxx_name = "w3"]
        W3 = 171,
        #[cxx_name = "w4"]
        W4 = 172,
        #[cxx_name = "w5"]
        W5 = 173,
        #[cxx_name = "w6"]
        W6 = 174,
        #[cxx_name = "w7"]
        W7 = 175,
        #[cxx_name = "w8"]
        W8 = 176,
        #[cxx_name = "w9"]
        W9 = 177,
        #[cxx_name = "w10"]
        W10 = 178,
        #[cxx_name = "w11"]
        W11 = 179,
        #[cxx_name = "w12"]
        W12 = 180,
        #[cxx_name = "w13"]
        W13 = 181,
        #[cxx_name = "w14"]
        W14 = 182,
        #[cxx_name = "w15"]
        W15 = 183,
        #[cxx_name = "w16"]
        W16 = 184,
        #[cxx_name = "w17"]
        W17 = 185,
        #[cxx_name = "w18"]
        W18 = 186,
        #[cxx_name = "w19"]
        W19 = 187,
        #[cxx_name = "w20"]
        W20 = 188,
        #[cxx_name = "w21"]
        W21 = 189,
        #[cxx_name = "w22"]
        W22 = 190,
        #[cxx_name = "w23"]
        W23 = 191,
        #[cxx_name = "w24"]
        W24 = 192,
        #[cxx_name = "w25"]
        W25 = 193,
        #[cxx_name = "w26"]
        W26 = 194,
        #[cxx_name = "w27"]
        W27 = 195,
        #[cxx_name = "w28"]
        W28 = 196,
        #[cxx_name = "w29"]
        W29 = 197,
        #[cxx_name = "w30"]
        W30 = 198,
        #[cxx_name = "x0"]
        X0 = 199,
        #[cxx_name = "x1"]
        X1 = 200,
        #[cxx_name = "x2"]
        X2 = 201,
        #[cxx_name = "x3"]
        X3 = 202,
        #[cxx_name = "x4"]
        X4 = 203,
        #[cxx_name = "x5"]
        X5 = 204,
        #[cxx_name = "x6"]
        X6 = 205,
        #[cxx_name = "x7"]
        X7 = 206,
        #[cxx_name = "x8"]
        X8 = 207,
        #[cxx_name = "x9"]
        X9 = 208,
        #[cxx_name = "x10"]
        X10 = 209,
        #[cxx_name = "x11"]
        X11 = 210,
        #[cxx_name = "x12"]
        X12 = 211,
        #[cxx_name = "x13"]
        X13 = 212,
        #[cxx_name = "x14"]
        X14 = 213,
        #[cxx_name = "x15"]
        X15 = 214,
        #[cxx_name = "x16"]
        X16 = 215,
        #[cxx_name = "x17"]
        X17 = 216,
        #[cxx_name = "x18"]
        X18 = 217,
        #[cxx_name = "x19"]
        X19 = 218,
        #[cxx_name = "x20"]
        X20 = 219,
        #[cxx_name = "x21"]
        X21 = 220,
        #[cxx_name = "x22"]
        X22 = 221,
        #[cxx_name = "x23"]
        X23 = 222,
        #[cxx_name = "x24"]
        X24 = 223,
        #[cxx_name = "x25"]
        X25 = 224,
        #[cxx_name = "x26"]
        X26 = 225,
        #[cxx_name = "x27"]
        X27 = 226,
        #[cxx_name = "x28"]
        X28 = 227,
        #[cxx_name = "v0"]
        V0 = 228,
        #[cxx_name = "v1"]
        V1 = 229,
        #[cxx_name = "v2"]
        V2 = 230,
        #[cxx_name = "v3"]
        V3 = 231,
        #[cxx_name = "v4"]
        V4 = 232,
        #[cxx_name = "v5"]
        V5 = 233,
        #[cxx_name = "v6"]
        V6 = 234,
        #[cxx_name = "v7"]
        V7 = 235,
        #[cxx_name = "v8"]
        V8 = 236,
        #[cxx_name = "v9"]
        V9 = 237,
        #[cxx_name = "v10"]
        V10 = 238,
        #[cxx_name = "v11"]
        V11 = 239,
        #[cxx_name = "v12"]
        V12 = 240,
        #[cxx_name = "v13"]
        V13 = 241,
        #[cxx_name = "v14"]
        V14 = 242,
        #[cxx_name = "v15"]
        V15 = 243,
        #[cxx_name = "v16"]
        V16 = 244,
        #[cxx_name = "v17"]
        V17 = 245,
        #[cxx_name = "v18"]
        V18 = 246,
        #[cxx_name = "v19"]
        V19 = 247,
        #[cxx_name = "v20"]
        V20 = 248,
        #[cxx_name = "v21"]
        V21 = 249,
        #[cxx_name = "v22"]
        V22 = 250,
        #[cxx_name = "v23"]
        V23 = 251,
        #[cxx_name = "v24"]
        V24 = 252,
        #[cxx_name = "v25"]
        V25 = 253,
        #[cxx_name = "v26"]
        V26 = 254,
        #[cxx_name = "v27"]
        V27 = 255,
        #[cxx_name = "v28"]
        V28 = 256,
        #[cxx_name = "v29"]
        V29 = 257,
        #[cxx_name = "v30"]
        V30 = 258,
        #[cxx_name = "v31"]
        V31 = 259,
        #[cxx_name = "pc"]
        Pc = 260,
        #[cxx_name = "tpidr_el0"]
        TpidrEl0 = 262,
        #[cxx_name = "tpidrro_el0"]
        TpidrroEl0 = 263,
        #[cxx_name = "pstate"]
        Pstate = 265,
        #[cxx_name = "fpcr"]
        Fpcr = 291,
        #[cxx_name = "fpsr"]
        Fpsr = 292,
        #[cxx_name = "bcr0"]
        Bcr0 = 4096,
        #[cxx_name = "bcr1"]
        Bcr1 = 4097,
        #[cxx_name = "bcr2"]
        Bcr2 = 4098,
        #[cxx_name = "bcr3"]
        Bcr3 = 4099,
        #[cxx_name = "bcr4"]
        Bcr4 = 4100,
        #[cxx_name = "bcr5"]
        Bcr5 = 4101,
        #[cxx_name = "bcr6"]
        Bcr6 = 4102,
        #[cxx_name = "bcr7"]
        Bcr7 = 4103,
        #[cxx_name = "bvr0"]
        Bvr0 = 4104,
        #[cxx_name = "bvr1"]
        Bvr1 = 4105,
        #[cxx_name = "bvr2"]
        Bvr2 = 4106,
        #[cxx_name = "bvr3"]
        Bvr3 = 4107,
        #[cxx_name = "bvr4"]
        Bvr4 = 4108,
        #[cxx_name = "bvr5"]
        Bvr5 = 4109,
        #[cxx_name = "bvr6"]
        Bvr6 = 4110,
        #[cxx_name = "bvr7"]
        Bvr7 = 4111,
        #[cxx_name = "wcr0"]
        Wcr0 = 4112,
        #[cxx_name = "wcr1"]
        Wcr1 = 4113,
        #[cxx_name = "wvr0"]
        Wvr0 = 4114,
        #[cxx_name = "wvr1"]
        Wvr1 = 4115,
        #[cxx_name = "midr_el1"]
        MidrEl1 = 4352,
        #[cxx_name = "id_aa64isar0_el1"]
        IdAa64isar0El1 = 4353,
        #[cxx_name = "id_aa64isar1_el1"]
        IdAa64isar1El1 = 4354,
    }

    struct DescriptorTableRegister {
        base: u64,
        limit: u32,
    }

    extern "Rust" {
        type Callback;

        fn invoke_callback(
            callback: &Callback,
            a: u64,
            b: u64,
            c: u64,
            d: u64,
            first: &[u8],
            second: &[u8],
        ) -> u64;
    }

    unsafe extern "C++" {
        include!("cpp/common.hpp");

        #[namespace = "sogen"]
        #[cxx_name = "backend_type"]
        type BackendType;
        #[namespace = "sogen"]
        #[cxx_name = "memory_permission"]
        type MemoryPermission;
        #[namespace = "sogen"]
        #[cxx_name = "stop_reason"]
        type StopReason;
        #[namespace = "sogen"]
        #[cxx_name = "instruction_hook_continuation"]
        type InstructionHookContinuation;
        #[namespace = "sogen"]
        #[cxx_name = "memory_violation_continuation"]
        type MemoryViolationContinuation;
        #[namespace = "sogen"]
        #[cxx_name = "memory_violation_type"]
        type MemoryViolationType;
        #[namespace = "sogen"]
        #[cxx_name = "memory_region_kind"]
        type MemoryRegionKind;
        #[namespace = "sogen"]
        #[cxx_name = "x86_hookable_instructions"]
        type X86HookableInstruction;
        #[namespace = "sogen"]
        #[cxx_name = "arm64_hookable_instructions"]
        type Arm64HookableInstruction;
        #[namespace = "sogen"]
        #[cxx_name = "x86_register"]
        type X86Register;
        #[namespace = "sogen"]
        #[cxx_name = "arm64_register"]
        type Arm64Register;
        #[namespace = "sogen"]
        #[cxx_name = "color"]
        type LogLevel;
        #[cxx_name = "memory_execution_hook_mode"]
        type MemoryExecutionHookMode;

        type CallbackHandle;
        type Machine;

        fn new_callback(callback: Box<Callback>) -> UniquePtr<CallbackHandle>;

        fn backend_name(self: &Machine) -> String;
        fn supports_instruction_counting(self: &Machine) -> bool;
        fn is_stop_thread_safe(self: &Machine) -> bool;
        fn supports_global_memory_execution_hooks(self: &Machine) -> bool;
        fn supports_multiple_vcpus(self: &Machine) -> bool;
        fn set_memory_execution_hook_mode(
            self: &Machine,
            mode: MemoryExecutionHookMode,
        ) -> Result<()>;
        fn stop(self: &Machine);

        fn read_memory(self: &Machine, address: u64, data: &mut [u8]) -> Result<()>;
        fn try_read_memory(self: &Machine, address: u64, data: &mut [u8]) -> bool;
        fn write_memory(self: &Machine, address: u64, data: &[u8]) -> Result<()>;
        fn try_write_memory(self: &Machine, address: u64, data: &[u8]) -> bool;

        fn read_x86_register(self: &Machine, reg: X86Register, value: &mut [u8]) -> Result<usize>;
        fn write_x86_register(self: &Machine, reg: X86Register, value: &[u8]) -> Result<usize>;
        fn read_arm64_register(
            self: &Machine,
            reg: Arm64Register,
            value: &mut [u8],
        ) -> Result<usize>;
        fn write_arm64_register(self: &Machine, reg: Arm64Register, value: &[u8]) -> Result<usize>;
        fn save_registers(self: &Machine) -> Result<Vec<u8>>;
        fn restore_registers(self: &Machine, data: &[u8]) -> Result<()>;
        fn segment_base(self: &Machine, segment: X86Register) -> Result<u64>;
        fn set_segment_base(self: &Machine, segment: X86Register, base: u64) -> Result<()>;
        fn descriptor_table(self: &Machine, table: X86Register) -> Result<DescriptorTableRegister>;
        fn load_gdt(self: &Machine, address: u64, limit: u32) -> Result<()>;

        fn hook_memory_execution(
            self: &Machine,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_memory_execution_at(
            self: &Machine,
            address: u64,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_memory_range_execution(
            self: &Machine,
            address: u64,
            size: u64,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_memory_read(
            self: &Machine,
            address: u64,
            size: u64,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_memory_write(
            self: &Machine,
            address: u64,
            size: u64,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_instruction(
            self: &Machine,
            instruction: i32,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_interrupt(self: &Machine, callback: UniquePtr<CallbackHandle>) -> Result<u64>;
        fn hook_memory_violation(
            self: &Machine,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
        fn hook_basic_block(self: &Machine, callback: UniquePtr<CallbackHandle>) -> Result<u64>;
        fn remove_hook(self: &Machine, id: u64) -> bool;
    }
}

/// Arguments of a native event. The meaning of each field depends on the event the callback was registered for.
pub struct CallbackArgs<'a> {
    pub values: [u64; 4],
    pub first: &'a [u8],
    pub second: &'a [u8],
}

type CallbackFunction = dyn Fn(&CallbackArgs<'_>) -> u64 + Send + Sync;

pub struct Callback(Box<CallbackFunction>);

impl Callback {
    pub fn handle(
        function: impl Fn(&CallbackArgs<'_>) -> u64 + Send + Sync + 'static,
    ) -> cxx::UniquePtr<ffi::CallbackHandle> {
        ffi::new_callback(Box::new(Self(Box::new(function))))
    }
}

fn invoke_callback(
    callback: &Callback,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    first: &[u8],
    second: &[u8],
) -> u64 {
    callback.0(&CallbackArgs {
        values: [a, b, c, d],
        first,
        second,
    })
}
