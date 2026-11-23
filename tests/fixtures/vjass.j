// @dialect vjass
library Counters initializer Init requires Utils, optional TimerUtils
    globals
        private integer count = 0
        public constant integer MAX = 8
        integer array sized[100]
    endglobals

    private keyword Ahead
    function interface Callback takes integer x returns nothing
    type IntList extends integer array [16]

    interface Shape
        method area takes nothing returns real
        method name takes nothing returns string defaults "shape"
    endinterface

    module Pooled
        static method get takes nothing returns thistype
            return thistype.allocate()
        endmethod
    endmodule

    struct Counter[20000] extends Shape
        integer value
        static integer made = 0
        private real ratio
        readonly integer id
        integer array list[4]
        implement Pooled
        implement optional Extra

        static method create takes integer v returns thistype
            local thistype c = thistype.allocate()
            set c.value = v
            set c.list[1] = c.list[0] + 2
            set thistype.made = thistype.made + 1
            set .id = thistype.made
            return c
        endmethod

        method area takes nothing returns real
            return this.value * 2.
        endmethod

        method operator [] takes integer i returns integer
            return .list[i]
        endmethod

        method operator x= takes integer v returns nothing
            set this.value = v
        endmethod

        stub method onDestroy takes nothing returns nothing
            call super.onDestroy()
        endmethod
    endstruct

    scope Inner initializer Start
        private function Start takes nothing returns nothing
            local Counter c = Counter.create(1)
            call c.destroy()
            call TimerStart(CreateTimer(), 1., false, function Counter.tick)
            static if LIBRARY_TimerUtils then
                call Callback.evaluate(c.area())
            else
                set count = Counters_MAX
            endif
        endfunction
    endscope

    private function Init takes nothing returns nothing
        set count = Counter(5).value
    endfunction
endlibrary
