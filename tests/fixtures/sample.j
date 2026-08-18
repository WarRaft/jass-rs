globals
    constant integer MAX_COUNT = 10
    integer array gCounters
    boolean gInitialized = false
endglobals

native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native GetLocalPlayer takes nothing returns player

function IsEven takes integer value returns boolean
    return value - (value / 2) * 2 == 0
endfunction

function InitCounters takes nothing returns nothing
    local integer i = 0
    loop
        exitwhen i >= MAX_COUNT
        set gCounters[i] = 0
        set i = i + 1
    endloop
    set gInitialized = true
endfunction

function SumCounters takes nothing returns integer
    local integer i = 0
    local integer total = 0
    loop
        exitwhen i >= MAX_COUNT
        set total = total + gCounters[i]
        set i = i + 1
    endloop
    return total
endfunction

function Main takes nothing returns nothing
    call InitCounters()
    if IsEven(SumCounters()) then
        call DisplayTextToPlayer(GetLocalPlayer(), 0, 0, "even")
    else
        call DisplayTextToPlayer(GetLocalPlayer(), 0, 0, "odd")
    endif
endfunction
